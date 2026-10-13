import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

type Pending = { resolve: (value: any) => void; reject: (error: Error) => void };

export default function (pi: ExtensionAPI) {
	const command = process.env.KOOLADE_MCP_COMMAND;
	const rawArgs = process.env.KOOLADE_MCP_ARGS;
	const rawTools = process.env.KOOLADE_MCP_TOOLS;
	if (!command || !rawArgs || !rawTools) throw new Error("Kool.ad/e application MCP session is unavailable");
	const args = JSON.parse(rawArgs) as string[];
	const allowedTools = new Set(JSON.parse(rawTools) as string[]);
	const server = spawn(command, args, { stdio: ["pipe", "pipe", "ignore"] });
	const pending = new Map<number, Pending>();
	const lines = createInterface({ input: server.stdout });
	let nextId = 0;
	let initialized = false;
	let startup: Promise<void> | undefined;
	lines.on("line", line => {
		let message: any;
		try { message = JSON.parse(line); } catch { return; }
		if (typeof message.id !== "number") return;
		const item = pending.get(message.id);
		if (!item) return;
		pending.delete(message.id);
		if (message.error) item.reject(new Error(String(message.error.message ?? "Application MCP request failed")));
		else item.resolve(message.result);
	});
	server.on("error", error => {
		for (const item of pending.values()) item.reject(error);
		pending.clear();
	});
	server.on("close", code => {
		const error = new Error(`Application MCP session closed (${code ?? "unknown"})`);
		for (const item of pending.values()) item.reject(error);
		pending.clear();
	});

	function request(method: string, params: unknown = {}): Promise<any> {
		const id = ++nextId;
		return new Promise((resolve, reject) => {
			pending.set(id, { resolve, reject });
			server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`, error => {
				if (error) {
					pending.delete(id);
					reject(error);
				}
			});
		});
	}

	async function initialize() {
		if (initialized) return;
		if (!startup) {
			startup = (async () => {
				await request("initialize", { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "koolade-pi-extension", version: "1" } });
				server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
				initialized = true;
			})();
		}
		await startup;
	}

	function register(name: string, description: string, parameters: any) {
		pi.registerTool({
			name,
			label: name.replaceAll("_", " "),
			description,
			parameters,
			async execute(_toolCallId, input) {
				try {
					await initialize();
					const result = await request("tools/call", { name, arguments: input });
					const text = Array.isArray(result?.content)
						? result.content.map((item: any) => item?.text ?? "").filter(Boolean).join("\n")
						: "Application MCP returned no tool result";
					return { content: [{ type: "text", text }], details: result, isError: result?.isError === true };
				} catch (error) {
					return { content: [{ type: "text", text: `Kool.ad/e execution session failed: ${String(error)}` }], isError: true };
				}
			},
		});
	}

	if (allowedTools.has("koolade_bash")) register("koolade_bash", "Run a bounded command through Kool.ad/e's Docker execution session. No host repository tools are available.", Type.Object({
		command: Type.String({ description: "Shell command for the application-selected workspace." }),
		timeout_seconds: Type.Optional(Type.Number({ minimum: 1, maximum: 1200 })),
	}));
	if (allowedTools.has("koolade_resource")) register("koolade_resource", "Request one public non-package HTTPS resource through Kool.ad/e.", Type.Object({
		url: Type.String(), purpose: Type.String(),
	}));
	if (allowedTools.has("koolade_dependency")) register("koolade_dependency", "Submit a package need for application-owned authorization and preparation.", Type.Object({
		ecosystem: Type.String(), package: Type.Optional(Type.String()), version: Type.Optional(Type.String()),
		source: Type.Optional(Type.String()), command: Type.String(), reason: Type.String(), kind: Type.String(),
	}));

	pi.on("session_shutdown", () => {
		server.stdin.end();
		server.kill("SIGTERM");
	});
}
