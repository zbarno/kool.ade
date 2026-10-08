import { spawn } from "node:child_process";
import { connect } from "node:net";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { disableShellGlobbing, prepareThenRetry, withPreparedNpmCacheIndex } from "./dependency_retry.mjs";
import { dependencyOperation, isDotnetRestoreCommand } from "./dependency_operation.mjs";
import type { DependencyKind, DependencyNeed, PackageEcosystem } from "./dependency_operation.mjs";

const OUTPUT_LIMIT = 50_000;
const DEFAULT_TIMEOUT_SECONDS = 300;
const MAX_TIMEOUT_SECONDS = 1_200;

type Sandbox = { bwrap: string; root: string; args: string[] };

type ResourceResponse = { status: string; summary: string; content?: string; path?: string; dependency_request?: unknown };
type BrokerRequest = {
	action?: "prepare_npm" | "prepare_nuget_audit" | "unsupported_manager" | "dependency_request";
	manager?: string;
	url?: string;
	dependency?: DependencyNeed;
	purpose: string;
};

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
	offlinePackageManagers = false,
): Promise<{ text: string; details: { exitCode: number | null; signal: string | null; timedOut: boolean }; isError: boolean }> {
	const child = spawn(sandbox.bwrap, [
		...sandbox.args,
		...(offlinePackageManagers ? [
			"--setenv", "npm_config_offline", "true",
			"--setenv", "npm_config_registry", "https://registry.npmjs.org/",
			"--setenv", "npm_config_userconfig", "/dev/null",
			"--setenv", "npm_config_globalconfig", "/tmp/koolade-home/.npm-globalrc",
			"--setenv", "npm_config_audit", "false",
			"--setenv", "CARGO_NET_OFFLINE", "true",
		] : []),
		"--", "/bin/bash", "-c",
		'ulimit -u 1024; ulimit -f 2097152; ulimit -c 0; exec /bin/bash -c "$1"',
		"koolade-sandbox", offlinePackageManagers
			? withPreparedNpmCacheIndex(disableShellGlobbing(command))
			: command,
	], {
		cwd: sandbox.root,
		env: {
		PATH: "/usr/bin:/bin",
		HOME: "/tmp/koolade-home",
		},
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

export default function (pi: ExtensionAPI) {
	const raw = process.env.KOOLADE_SANDBOX_CONFIG;
	if (!raw) throw new Error("Koolade sandbox configuration is missing");
	const sandbox = JSON.parse(raw) as Sandbox;
	const resourceSocket = process.env.KOOLADE_RESOURCE_SOCKET;
	if (!resourceSocket) throw new Error("Koolade resource broker is unavailable");

	pi.registerTool({
		name: "koolade_resource",
		label: "request public resource",
		description: "Ask Kool.ad/e to retrieve a specific non-package public HTTPS resource. Package requests must use koolade_dependency so Man.ager and the ecosystem broker can apply deterministic source, integrity, authorization, and retry policy.",
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
		name: "koolade_dependency",
		label: "request dependency authorization",
		description: "Submit a specific package need to Kool.ad/e Man.ager. Kool.ad/e supplies the task identity and evaluates authorization; this tool cannot grant package access or change sandbox permissions. Use it before attempting to add a project or development dependency, and provide its source and task-specific reason when known.",
		parameters: Type.Object({
			ecosystem: Type.String({ description: "Package manager: npm, pnpm, yarn, cargo, nuget, pip, uv, poetry, system, or other" }),
			package: Type.Optional(Type.String({ description: "Exact package name when known" })),
			version: Type.Optional(Type.String({ description: "Requested version or range when known" })),
			source: Type.Optional(Type.String({ description: "Registry or source URL when known" })),
			command: Type.String({ description: "The exact package-manager command that needs the package" }),
			reason: Type.String({ description: "Why this package is needed for the assigned task" }),
			kind: Type.String({ description: "existing_restore, new_project_dependency, development_dependency, or system_tool" }),
		}),
		async execute(_toolCallId, params) {
			const dependency: DependencyNeed = {
				ecosystem: params.ecosystem.toLowerCase() as PackageEcosystem,
				package: params.package,
				version: params.version,
				source: params.source,
				command: params.command,
				reason: params.reason,
				kind: params.kind.toLowerCase() as DependencyKind,
			};
			try {
				const execution = await prepareThenRetry(() => requestResource(resourceSocket, {
					action: "dependency_request",
					dependency,
					purpose: params.reason,
				}, 20 * 60_000), () => runSandbox(sandbox, dependency.command, DEFAULT_TIMEOUT_SECONDS, undefined, true));
				return {
					content: [{ type: "text", text: `${execution.preparation.summary}${execution.retry ? `\n\n${execution.retry.text}` : ""}` }],
					details: execution.retry ? { preparation: execution.preparation, retry: execution.retry.details } : execution.preparation,
					isError: execution.isError,
				};
			} catch (error) {
				return { content: [{ type: "text", text: `Dependency request failed: ${String(error)}` }], isError: true };
			}
		},
	});

	pi.registerTool({
		name: "koolade_bash",
		label: "bounded bash",
		description: "Run a shell command inside the assigned task worktree. The task folder is the only persistent writable location; host home and credentials are hidden, network is disabled, and Git publication is unavailable. Use this for implementation and verification commands.",
		parameters: Type.Object({
			command: Type.String({ description: "Bash command to run in the assigned worktree" }),
			timeout: Type.Optional(Type.Number({ minimum: 1, maximum: MAX_TIMEOUT_SECONDS, description: "Maximum seconds for this command" })),
		}),
		async execute(_toolCallId, params, signal) {
			if (/\b(?:pnpm\s+(?:install|i)|yarn\s+install)\b/.test(params.command)) {
				const manager = /\bpnpm\s+(?:install|i)\b/.test(params.command) ? "pnpm" : "yarn";
				try {
					const dependency = dependencyOperation(params.command);
					const execution = await prepareThenRetry(() => requestResource(resourceSocket, {
						action: "dependency_request",
						purpose: `The worker requested a ${manager} dependency install, which needs package-manager cache support`,
						dependency,
					}, 20 * 60_000), () => runSandbox(sandbox, params.command, Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS)), signal, true));
					return {
						content: [{ type: "text", text: `${execution.preparation.summary}${execution.retry ? `\n\n${execution.retry.text}` : ""}` }],
						details: execution.retry ? { preparation: execution.preparation, retry: execution.retry.details } : execution.preparation,
						isError: execution.isError,
					};
				} catch (error) {
					return { content: [{ type: "text", text: `Dependency preparation failed: ${String(error)}` }], isError: true };
				}
			}
			if (/\bnpm\s+(?:ci|install|i)\b/.test(params.command)) {
				const dependency = dependencyOperation(params.command);
				if (dependency && dependency.kind !== "existing_restore") {
					const execution = await prepareThenRetry(() => requestResource(resourceSocket, {
						action: "dependency_request",
						dependency,
						purpose: dependency.reason,
					}, 20 * 60_000), () => runSandbox(sandbox, params.command, Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS)), signal, true));
					return {
						content: [{ type: "text", text: `${execution.preparation.summary}${execution.retry ? `\n\n${execution.retry.text}` : ""}` }],
						details: execution.retry ? { preparation: execution.preparation, retry: execution.retry.details } : execution.preparation,
						isError: execution.isError,
					};
				}
				try {
					const prepared = await requestResource(resourceSocket, {
						action: "dependency_request",
						dependency,
						purpose: dependency?.reason ?? "Prepare the project-declared npm dependencies",
					}, 20 * 60_000);
					if (prepared.status !== "prepared") {
						return { content: [{ type: "text", text: prepared.summary }], details: prepared, isError: true };
					}
					const retried = await runSandbox(sandbox, params.command, Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS)), signal, true);
					return {
						content: [{ type: "text", text: `${prepared.summary}\n\n${retried.text}` }],
						details: { preparation: prepared, retry: retried.details },
						isError: retried.isError,
					};
				} catch (error) {
					return { content: [{ type: "text", text: `Dependency preparation failed: ${String(error)}` }], isError: true };
				}
			}
			const operation = dependencyOperation(params.command);
			if (operation && operation.ecosystem !== "npm") {
				try {
					const result = await requestResource(resourceSocket, {
						action: "dependency_request",
						dependency: operation,
						purpose: operation.reason,
					}, 20 * 60_000);
					if (result.status !== "prepared") {
						return { content: [{ type: "text", text: result.summary }], details: result, isError: true };
					}
					const retried = await runSandbox(sandbox, params.command, Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS)), signal, true);
					return {
						content: [{ type: "text", text: `${result.summary}\n\n${retried.text}` }],
						details: { preparation: result, retry: retried.details },
						isError: retried.isError,
					};
				} catch (error) {
					return { content: [{ type: "text", text: `Dependency request failed: ${String(error)}` }], isError: true };
				}
			}
			const timeout = Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS));
			let result = await runSandbox(sandbox, params.command, timeout, signal, /\bnpm\s+(?:ci|install|i)\b/.test(params.command));
			if (result.isError && isDotnetRestoreCommand(params.command) && /error\s+nu1900:/i.test(result.text)) {
				const refreshed = await requestResource(resourceSocket, {
					action: "prepare_nuget_audit",
					purpose: "Refresh public NuGet vulnerability metadata after the sandbox could not reach the audit feed",
				}, 100_000);
				if (refreshed.status === "prepared") {
					result = await runSandbox(sandbox, params.command, timeout, signal, true);
					result.text = `${refreshed.summary}\n\n${result.text}`;
				} else {
					result.text += `\n\nKool.ad/e could not refresh the public NuGet audit feed: ${refreshed.summary}`;
				}
			}
			return { content: [{ type: "text", text: result.text }], details: result.details, isError: result.isError };
		},
	});
}
