export async function prepareThenRetry(prepare, retry) {
	const preparation = await prepare();
	if (preparation.status !== "prepared") {
		return { preparation, isError: true };
	}
	const result = await retry();
	return { preparation, retry: result, isError: result.isError };
}

export function disableShellGlobbing(command) {
	return `set -f; ${command}`;
}

export function withPreparedNpmCacheIndex(command) {
	const sourceRoot = "/tmp/koolade-home/.npm-prepared/_cacache/index-source-v5";
	const destination = "/tmp/koolade-home/.npm-prepared/_cacache/index-v5";
	const temporary = "/tmp/koolade-home/.npm-prepared/_cacache/tmp";
	return `mkdir -p ${destination} ${temporary} && ` +
		`generation=$(cat ${sourceRoot}/current) && ` +
		`case "$generation" in ''|*[!0-9a-f-]*) ` +
		`printf '%s\\n' 'Prepared npm snapshot pointer is invalid.' >&2; exit 1;; esac && ` +
		`test "\${#generation}" = 36 && ` +
		`source=${sourceRoot}/$generation/index-v5 && ` +
		`if find "$source" -type l -print -quit | grep -q .; then ` +
		`printf '%s\\n' 'Prepared npm index contains a symlink.' >&2; exit 1; fi && ` +
		`cp -R "$source"/. ${destination}/ && ${command}`;
}
