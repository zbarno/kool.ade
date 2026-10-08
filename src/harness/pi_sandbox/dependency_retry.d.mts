export type PreparationResponse = { status: string; summary: string };
export type RetryResult = { text: string; details: unknown; isError: boolean };

export function prepareThenRetry<T extends PreparationResponse>(
	prepare: () => Promise<T>,
	retry: () => Promise<RetryResult>,
): Promise<{ preparation: T; retry?: RetryResult; isError: boolean }>;

export function disableShellGlobbing(command: string): string;
