export type PackageEcosystem = "npm" | "pnpm" | "yarn" | "cargo" | "nuget" | "pip" | "uv" | "poetry" | "system" | "other";
export type DependencyKind = "existing_restore" | "new_project_dependency" | "development_dependency" | "system_tool";
export type DependencyNeed = {
	ecosystem: PackageEcosystem;
	package?: string;
	version?: string;
	source?: string;
	command: string;
	reason: string;
	kind: DependencyKind;
};

export function dependencyOperation(command: string): DependencyNeed | undefined;
export function isDotnetRestoreCommand(command: string): boolean;
