import { spawnSync } from "node:child_process";

function git(checkout: string, args: string[]): string {
  const result = spawnSync("git", ["-C", checkout, ...args], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`cannot inspect asset-tooling checkout: ${result.stderr.trim()}`);
  }
  return result.stdout.trim();
}

/** Never attribute bytes from a modified worktree to its HEAD commit. */
export function assertCleanSourceCheckout(checkout: string, expectedCommit: string): void {
  if (git(checkout, ["rev-parse", "HEAD"]) !== expectedCommit) {
    throw new Error(`asset-tooling checkout must be at ${expectedCommit}`);
  }
  if (git(checkout, ["status", "--porcelain", "--untracked-files=all"])) {
    throw new Error("asset-tooling source checkout must be clean before packaging");
  }
}
