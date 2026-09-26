import { spawn } from "node:child_process";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const OUTPUT_LIMIT = 50_000;
const DEFAULT_TIMEOUT_SECONDS = 300;
const MAX_TIMEOUT_SECONDS = 1_200;

type Sandbox = { bwrap: string; root: string; args: string[] };

function appendTail(current: string, chunk: string): string {
	const joined = current + chunk;
	return joined.length <= OUTPUT_LIMIT ? joined : joined.slice(-OUTPUT_LIMIT);
}

export default function (pi: ExtensionAPI) {
	const raw = process.env.PACKET_SANDBOX_CONFIG;
	if (!raw) throw new Error("Packet sandbox configuration is missing");
	const sandbox = JSON.parse(raw) as Sandbox;

	pi.registerTool({
		name: "packet_bash",
		label: "bounded bash",
		description: "Run a shell command inside the assigned task worktree. The task folder is the only persistent writable location; host home and credentials are hidden, network is disabled, and Git publication is unavailable. Use this for implementation and verification commands.",
		parameters: Type.Object({
			command: Type.String({ description: "Bash command to run in the assigned worktree" }),
			timeout: Type.Optional(Type.Number({ minimum: 1, maximum: MAX_TIMEOUT_SECONDS, description: "Maximum seconds for this command" })),
		}),
		async execute(_toolCallId, params, signal) {
			const timeout = Math.min(MAX_TIMEOUT_SECONDS, Math.max(1, params.timeout ?? DEFAULT_TIMEOUT_SECONDS));
			const child = spawn(sandbox.bwrap, [
				...sandbox.args,
				"--", "/bin/bash", "-c",
				'ulimit -u 128; ulimit -f 2097152; ulimit -c 0; exec /bin/bash -c "$1"',
				"packet-sandbox", params.command,
			], {
				cwd: sandbox.root,
				env: { PATH: "/usr/bin:/bin", HOME: "/tmp/packet-home" },
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
				return { content: [{ type: "text", text }], details: { exitCode: result.code, signal: result.signal, timedOut }, isError: timedOut || result.code !== 0 };
			} finally {
				clearTimeout(timer);
				signal?.removeEventListener("abort", abort);
			}
		},
	});
}
