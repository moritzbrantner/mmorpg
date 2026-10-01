import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { assertCleanSourceCheckout } from "../scripts/asset-source-checkout";

describe("asset source provenance", () => {
  test("rejects tracked edits and untracked source while accepting an ignored build output", () => {
    const checkout = mkdtempSync(path.join(tmpdir(), "mmorpg-asset-source-"));
    const git = (...args: string[]) => {
      const result = spawnSync("git", ["-C", checkout, ...args], { encoding: "utf8" });
      expect(result.status).toBe(0);
      return result.stdout.trim();
    };
    try {
      git("init", "-q");
      writeFileSync(path.join(checkout, ".gitignore"), "build/\n");
      writeFileSync(path.join(checkout, "generator.ts"), "export const version = 1;\n");
      git("add", ".");
      git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "source");
      const commit = git("rev-parse", "HEAD");
      mkdirSync(path.join(checkout, "build"));
      writeFileSync(path.join(checkout, "build", "output.obj"), "disposable\n");
      assertCleanSourceCheckout(checkout, commit);
      writeFileSync(path.join(checkout, "generator.ts"), "export const version = 2;\n");
      expect(() => assertCleanSourceCheckout(checkout, commit)).toThrow("must be clean");
      git("checkout", "--", "generator.ts");
      writeFileSync(path.join(checkout, "shadow.js"), "export const version = 2;\n");
      expect(() => assertCleanSourceCheckout(checkout, commit)).toThrow("must be clean");
      rmSync(path.join(checkout, "shadow.js"));
      expect(() => assertCleanSourceCheckout(checkout, "0000000000000000000000000000000000000000")).toThrow("must be at");
    } finally {
      rmSync(checkout, { recursive: true, force: true });
    }
  });
});
