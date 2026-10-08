import { spawn } from "node:child_process";
import { connect } from "node:net";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const OUTPUT_LIMIT = 50_000;
const DEFAULT_TIMEOUT_SECONDS = 300;
const MAX_TIMEOUT_SECONDS = 1_200;

type Sandbox = { bwrap: string; root: string; args: string[] };

type ResourceResponse = { status: string; summary: string; content?: string; path?: string };
type BrokerRequest = { action?: "prepare_npm" | "prepare_nuget_audit" | "unsupported_manager"; manager?: string; url?: string; purpose: string };

function requestResource(socketPath: string, request: BrokerRequest, timeoutMs = 35_000): Promise<ResourceResponse> {
	return new Promise((resolve, reject) => {
		const socket = connect(socketPath);
		let response = "";
		socket.setTimeout(timeoutMs, () => socket.destroy(new Error("Resource request timed out")));
		socket.on("connect", () => socket.end(JSON.stringify(request) + "\n"));
		socket.on("data", (chunk: Buffer) => {
			response += chunk.toString("utf8");
			if (response.length > 1_000_000) socket.destroy(new Error("Resource response is too large"));
			const newline = response.indexOf("\n");
			if (newline >= 0) {
				try { resolve(JSON.parse(response.slice(0, newline)) as ResourceResponse); }
				catch (error) { reject(error); }
				socket.end();
			}
		});
		socket.on("error", reject);
	});
}

function appendTail(current: string, chunk: string): string {
	const joined = current + chunk;
	return joined.length <= OUTPUT_LIMIT ? joined : joined.slice(-OUTPUT_LIMIT);
}

async function runSandbox(
	sandbox: Sandbox,
	command: string,
	timeout: number,
	signal?: AbortSignal,
): Promise<{ text: string; details: { exitCode: number | null; signal: string | null; timedOut: boolean }; isError: boolean }> {
	const child = spawn(sandbox.bwrap, [
		...sandbox.args,
		"--", "/bin/bash", "-c",
		'ulimit -u 1024; ulimit -f 2097152; ulimit -c 0; exec /bin/bash -c "$1"',
		"koolade-sandbox", command,
	], {
		cwd: sandbox.root,
		env: { PATH: "/usr/bin:/bin", HOME: "/tmp/koolade-home" },
		stdio: ["ignore", "pipe", "pipe"],
	});

	let stdout = "";
	let stderr = "";
	let timedOut = false;
	child.stdout?.on("data", (data: Buffer) => { stdout = appendTail(stdout, data.toString("utf8")); });
	child.stderr?.on("data", (data: Buffer) => { stderr = appendTail(stderr, data.toString("utf8")); });
	const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, timeout * 1_000);
	const abort = () => child.kill("SIGKILL");
	if (signal?.aborted) child.kill("SIGKILL");
	else signal?.addEventListener("abort", abort, { once: true });
	try {
		const result = await new Promise<{ code: number | null; signal: string | null }>((resolve, reject) => {
			child.once("error", reject);
			child.once("close", (code, childSignal) => resolve({ code, signal: childSignal }));
		});
		const text = [stdout.trim(), stderr.trim(), timedOut ? `Command timed out after ${timeout} seconds.` : "", `Exit code: ${result.code ?? result.signal ?? "unknown"}`]
			.filter(Boolean).join("\n");
		return { text, details: { exitCode: result.code, signal: result.signal, timedOut }, isError: timedOut || result.code !== 0 };
	} finally {
		clearTimeout(timer);
		signal?.removeEventListener("abort", abort);
	}
}

function isDotnetRestoreCommand(command: string): boolean {
	return /(?:^|[;&|\n])\s*(?:[^;&|\s]+\/)?dotnet\s+(?:restore|build|test|publish)\b/i.test(command);
}

export default function (pi: ExtensionAPI) {
	const raw = process.env.KOOLADE_SANDBOX_CONFIG;
	if (!raw) throw new Error("Koolade sandbox configuration is missing");
	const sandbox = JSON.parse(raw) as Sandbox;
	const resourceSocket = process.env.KOOLADE_RESOURCE_SOCKET;
	if (!resourceSocket) throw new Error("Koolade resource broker is unavailable");

	pi.registerTool({
		name: "koolade_resource",
		label: "request public resource",
		description: "Ask Kool.ad/e to retrieve a specific public HTTPS resource. Lockfile-pinned public npm archives and the official NuGet vulnerability cache can be prepared for sandbox retries. Other sources and unsupported package managers are surfaced to the operator as Needs Attention.",
		parameters: Type.Object({
			url: Type.String({ description: "Exact HTTPS URL of the needed resource" }),
			purpose: Type.String({ description: "Brief explanation of why the task needs this resource" }),
		}),
		async execute(_toolCallId, params) {
			try {
				const result = await requestResource(resourceSocket, { url: params.url, purpose: params.purpose });
				const text = [result.summary, result.content, result.path ? `Saved file: ${result.path}` : undefined].filter(Boolean).join("\n");
				return { content: [{ type: "text", text }], details: result, isError: result.status !== "allowed" };
			} catch (error) {
				return { content: [{ type: "text", text: `Resource request failed: ${String(error)}` }], isError: true };
			}
		},
	});

	pi.registerTool({
		name: "koolade_bash",
		label: "bounded bash",
		description: "Run a shell command inside the assigned task repository. Its full clone is the only persistent writable location; host home and credentials are hidden, network is disabled, and Git publication is unavailable. Use this for implementation and verification commands.",
		parameters: Type.Object({
			command: Type.String({ description: "Bash command to run in the assigned task repository" }),
			timeout: Type.Optional(Type.Number({ minimum: 1, maximum: MAX_TIMEOUT_SECONDS, description: "Maximum seconds for this command" })),
		}),
		async execute(_toolCallId, params, signal) {
			if (/\b(?:pnpm\s+(?:install|i)|yarn\s+install)\b/.test(params.command)) {
				const manager = /\bpnpm\s+(?:install|i)\b/.test(params.command) ? "pnpm" : "yarn";
				try {
					const result = await requestResource(resourceSocket, {
						action: "unsupported_manager",
						manager,
						purpose: `The worker requested a ${manager} dependency install, which needs package-manager cache support`,
					});
					return { content: [{ type: "text", text: result.summary }], details: result, isError: true };
				} catch (error) {
					return { content: [{ type: "text", text: `Dependency preparation failed: ${String(error)}` }], isError: true };
				}
			}
			if (/\bnpm\s+(?:ci|install|i)\b/.test(params.command)) {
				try {
					const prepared = await requestResource(resourceSocket, {
						action: "prepare_npm",
						purpose: "Prepare locked npm dependencies before the sandbox runs its offline package-manager command",
					}, 8 * 60_000);
					if (prepared.status !== "prepared") {
						return { content: [{ type: "text", text: prepared.summary }], details: prepared, isError: true };
					}
				} catch (error) {
					return { content: [{ type: "text", text: `Dependency preparation failed: ${String(error)}` }], isError: true };
				}
			}
			const timeout = Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS));
			let result = await runSandbox(sandbox, params.command, timeout, signal);
			if (result.isError && isDotnetRestoreCommand(params.command) && /error\s+nu1900:/i.test(result.text)) {
				const refreshed = await requestResource(resourceSocket, {
					action: "prepare_nuget_audit",
					purpose: "Refresh public NuGet vulnerability metadata after the sandbox could not reach the audit feed",
				}, 100_000);
				if (refreshed.status === "prepared") {
					result = await runSandbox(sandbox, params.command, timeout, signal);
					result.text = `${refreshed.summary}\n\n${result.text}`;
				} else {
					result.text += `\n\nKool.ad/e could not refresh the public NuGet audit feed: ${refreshed.summary}`;
				}
			}
			return { content: [{ type: "text", text: result.text }], details: result.details, isError: result.isError };
		},
	});
}
