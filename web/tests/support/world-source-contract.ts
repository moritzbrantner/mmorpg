import { expect, test } from "bun:test";
import type { WorldCommand } from "../../src/command-wire";
import type { EntityState, ZoneSnapshot } from "../../src/replication";
import type { WorldSource } from "../../src/world/world-source";

const ownUnit = (entities: readonly EntityState[], player: number) =>
  entities.find((entity) => entity.kind === "player" && entity.entityId === player);

/** A source whose joins are refused until `repair()`, e.g. because its first projection is rejected. */
export type RefusingSource = { source: WorldSource; repair(): void };

/** Behaviour every `WorldSource` owes the presentation layer, local or online. */
export function worldSourceContract(
  name: string,
  create: () => WorldSource,
  createRefusing: () => RefusingSource,
): void {
  test(`${name}: a joined player receives projections addressed to it that contain it`, () => {
    const source = create();
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    const player = source.join();
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(player);
    expect(ownUnit(projection?.entities ?? [], player)).toBeDefined();
    expect(ownUnit(source.sample(), player)).toBeDefined();
  });

  test(`${name}: joining twice or commanding without a player fails closed`, () => {
    const source = create();
    const move: WorldCommand = { kind: "move", forward: 1, strafe: 0, facing: 0 };
    expect(() => source.sendCommand(move)).toThrow();
    source.join();
    expect(() => source.join()).toThrow();
    source.sendCommand(move);
    source.sendCommand({ kind: "jump" });
    source.leave();
    expect(() => source.sendCommand(move)).toThrow();
    source.leave();
  });

  test(`${name}: advancing time moves projections forward and never back`, () => {
    const source = create();
    source.join();
    const ticks: bigint[] = [];
    const record = () => ticks.push((source.latestProjection() as ZoneSnapshot).tick);
    record();
    for (const seconds of [0, 0.01, 0.05, 0.05, 0.2]) {
      source.advance(seconds);
      record();
    }
    expect(ticks.at(-1)! > ticks[0]!).toBe(true);
    for (let index = 1; index < ticks.length; index += 1) {
      expect(ticks[index]! >= ticks[index - 1]!).toBe(true);
    }
  });

  test(`${name}: a refused join leaves the source unjoined so entry can be retried`, () => {
    const { source, repair } = createRefusing();
    expect(() => source.join()).toThrow();
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    expect(() => source.sendCommand({ kind: "jump" })).toThrow();
    source.advance(0.1);
    expect(source.latestProjection()).toBeNull();
    source.leave();
    repair();
    const player = source.join();
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(player);
    // The refused entry left no unit behind in the world.
    expect(projection?.entities.map((entity) => entity.entityId)).toEqual([player]);
    source.sendCommand({ kind: "jump" });
  });

  test(`${name}: leaving clears presentation and the next join is a new player`, () => {
    const source = create();
    const first = source.join();
    source.advance(0.1);
    source.leave();
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    source.advance(0.1);
    expect(source.latestProjection()).toBeNull();
    const second = source.join();
    expect(second).not.toBe(first);
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(second);
    expect(ownUnit(projection?.entities ?? [], first)).toBeUndefined();
  });
}
