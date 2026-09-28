import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { encodeCommand, type Axis, type WorldCommand } from "../src/command-wire";

const fixture = readFileSync(new URL("../../fixtures/protocol/commands-v2.hex", import.meta.url), "utf8");

function axis(raw: string | undefined): Axis {
  const value = Number(raw);
  if (value !== -1 && value !== 0 && value !== 1) throw new Error(`Bad fixture axis ${raw}`);
  return value;
}

function fixtureCommands(): { hex: string; command: WorldCommand }[] {
  return fixture
    .split("\n")
    .filter((line) => line.length > 0 && !line.startsWith("#"))
    .map((line) => {
      const [hex, kind, forward, strafe, facing] = line.split(" ");
      if (!hex) throw new Error("Empty fixture line");
      if (kind === "jump") return { hex, command: { kind: "jump" } };
      if (kind !== "move") throw new Error(`Unknown fixture command ${kind}`);
      return { hex, command: { kind: "move", forward: axis(forward), strafe: axis(strafe), facing: Number(facing) } };
    });
}

describe("Rust/browser command contract", () => {
  test("encodes the same golden bytes as mmorpg-protocol", () => {
    const commands = fixtureCommands();
    expect(commands.map(({ command }) => command.kind)).toContain("jump");
    expect(commands.filter(({ command }) => command.kind === "move").length).toBeGreaterThanOrEqual(4);
    for (const { hex, command } of commands) {
      expect(Buffer.from(encodeCommand(command)).toString("hex")).toBe(hex);
    }
  });

  test("rejects intent the Rust decoder would reject", () => {
    for (const invalid of [
      { kind: "move", forward: 2, strafe: 0, facing: 0 },
      { kind: "move", forward: 0, strafe: -2, facing: 0 },
      { kind: "move", forward: 0, strafe: 0, facing: 65_536 },
      { kind: "move", forward: 0, strafe: 0, facing: -1 },
      { kind: "move", forward: 0, strafe: 0, facing: 1.5 },
    ]) {
      expect(() => encodeCommand(invalid as WorldCommand)).toThrow();
    }
  });
});
