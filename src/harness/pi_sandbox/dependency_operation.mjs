export function isDotnetRestoreCommand(command) {
	return /(?:^|[;&|\n])\s*(?:[^;&|\s]+\/)?dotnet\s+(?:restore|build|test|publish)\b/i.test(command);
}

export function dependencyOperation(command) {
	const registry = /(?:^|\s)--registry=(?:"([^"]+)"|'([^']+)'|([^\s]+))/i.exec(command);
	const requestedRegistry = registry?.[1] ?? registry?.[2] ?? registry?.[3];
	const npmInstall = /\bnpm\s+(?:install|i)\s+([^;&|\n]+)/i.exec(command);
	if (npmInstall) {
		const spec = npmInstall[1].split(/\s+/).map((token) => token.replace(/^['"`]|['"`,;]$/g, ""))
			.find((token) => token && !token.startsWith("-"));
		if (spec) {
			const source = requestedRegistry ?? "https://registry.npmjs.org";
			const separator = spec.startsWith("@") ? spec.lastIndexOf("@") : spec.lastIndexOf("@");
			const hasVersion = separator > (spec.startsWith("@") ? spec.indexOf("/") : 0);
			const packageName = hasVersion ? spec.slice(0, separator) : spec;
			return {
				ecosystem: "npm",
				package: packageName,
				version: hasVersion ? spec.slice(separator + 1) : "latest",
				source,
				command,
				reason: "The task worker requested a new npm dependency; Man.ager will evaluate its fit for the assigned task.",
				kind: /(?:--save-dev|--dev|\s-D(?:\s|$))/.test(command) ? "development_dependency" : "new_project_dependency",
			};
		}
	}
	if (/\bnpm\s+(?:ci|install|i)\b/i.test(command)) {
		return {
			ecosystem: "npm",
			source: requestedRegistry ?? "https://registry.npmjs.org",
			command,
			reason: "Restore project-declared npm dependencies from the lockfile before the sandbox runs offline.",
			kind: "existing_restore",
		};
	}
	const addCases = [
		[/\bcargo\s+add\s+([^;&|\s]+)/i, "cargo", "new_project_dependency"],
		[/\bdotnet\s+add\s+[^;&|\s]+\s+package\s+([^;&|\s]+)/i, "nuget", "new_project_dependency"],
		[/\b(?:pip|pip3)\s+install\s+([^;&|\s]+)/i, "pip", "new_project_dependency"],
		[/\buv\s+add\s+([^;&|\s]+)/i, "uv", "new_project_dependency"],
		[/\bpoetry\s+add\s+([^;&|\s]+)/i, "poetry", "new_project_dependency"],
	];
	for (const [pattern, ecosystem] of addCases) {
		const match = pattern.exec(command);
		if (match) {
			return {
				ecosystem,
				package: match[1].replace(/^['"`]|['"`,;]$/g, ""),
				version: "unspecified",
				command,
				reason: "The task worker requested a new project dependency; Man.ager will evaluate its fit for the assigned task.",
				kind: "new_project_dependency",
			};
		}
	}
	if (/\bcargo\s+install\b/i.test(command)) {
		return {
			ecosystem: "system",
			command,
			reason: "The task worker requested a Cargo-installed host tool; host tool installation is not available inside a task sandbox.",
			kind: "system_tool",
		};
	}
	const manager = /\b(pnpm|yarn|cargo|dotnet|pip3?|uv|poetry)\s+(?:install|i|ci|fetch|restore|sync|build|check|test|clippy|doc|bench|update|publish)\b/i.exec(command)?.[1]?.toLowerCase();
	if (manager && !(manager === "dotnet" && isDotnetRestoreCommand(command) && /error\s+nu1900:/i.test(command))) {
		const ecosystem = manager === "pip3" ? "pip" : manager === "dotnet" ? "nuget" : manager;
		return {
			ecosystem,
			command,
			reason: `Restore or build the task project with ${manager}; Man.ager will check the dependency request.`,
			kind: manager === "cargo" && /\bcargo\s+update\b/i.test(command) ? "new_project_dependency" : "existing_restore",
		};
	}
	if (/\b(?:apt|apt-get|dnf|yum|pacman)\s+(?:install|add)\b/i.test(command)) {
		return {
			ecosystem: "system",
			command,
			reason: "The task worker requested a system tool; host package managers are never run inside the worker.",
			kind: "system_tool",
		};
	}
	return undefined;
}
