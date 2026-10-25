import { spawnSync } from "node:child_process";
import { copyFile, mkdtemp, mkdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = new URL(".", import.meta.url);
const installRoot = await mkdtemp(join(tmpdir(), "runtime-qa-npm-cache-"));
try {
  await copyFile(new URL("package.json", root), join(installRoot, "package.json"));
  await copyFile(new URL("package-lock.json", root), join(installRoot, "package-lock.json"));
  const install = spawnSync("npm", ["ci", "--offline", "--ignore-scripts", "--no-audit", "--no-fund"], {
    cwd: installRoot,
    encoding: "utf8",
  });
  if (install.status !== 0) {
    throw new Error(`the selected broker cache could not restore the task lockfile offline: ${install.stderr}`);
  }
  const packageContents = await readFile(
    join(installRoot, "node_modules/koolade-runtime-qa-cache-probe/index.js"),
    "utf8",
  );
  if (!packageContents.includes("prepared npm cache was installed offline")) {
    throw new Error("the installed cache probe does not match this task lockfile");
  }
} finally {
  await rm(installRoot, { recursive: true, force: true });
}

const output = new URL("dist/", root);
await mkdir(output, { recursive: true });
await copyFile(new URL("src/server.ts", root), new URL("server.ts", output));
await copyFile(new URL("src/index.html", root), new URL("index.html", output));
