import type { EntityRef } from "../../entity-ref";
import { findEntity, type ErrorCode, type ZoneEvent, type ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";

/**
 * Plain-text combat HUD lines for the minimal presentation; the target
 * frame, nameplates and combat text arrive with step 7b.
 */
const ERROR_TEXT: Record<ErrorCode, string> = {
  "no-target": "You have no target.",
  "out-of-range": "Out of range.",
  "target-dead": "Your target is dead.",
  "not-attackable": "You cannot attack that.",
  "you-are-dead": "You are dead.",
  "not-dead": "You are not dead.",
  "invalid-target": "Invalid target.",
};

/** A unit's display name: catalog names for creatures and NPCs, "you" for the viewer. */
export function unitName(entity: EntityRef, snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  switch (entity.kind) {
    case "player":
      return entity.id === snapshot.viewerId ? "you" : `Player ${entity.id}`;
    case "npc":
      return catalog.npcs.get(entity.id)?.name ?? `NPC ${entity.id}`;
    case "creature": {
      const template = findEntity(snapshot, entity)?.appearance;
      return (template === undefined ? undefined : catalog.creatureTemplates.get(template)?.name) ?? "a creature";
    }
  }
}

/** Health, level, combat state and target, or how to release a dead spirit. */
export function combatStatus(snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  const viewer = snapshot.viewer;
  if (viewer.dead) {
    return "You are dead. Press R to release your spirit to the graveyard.";
  }
  const parts = [`Health ${viewer.health}/${viewer.maxHealth}`, `Level ${viewer.level}`];
  if (viewer.inCombat) {
    parts.push("In combat");
  }
  if (viewer.target !== null) {
    const record = findEntity(snapshot, viewer.target);
    const name = unitName(viewer.target, snapshot, catalog);
    const detail = record === undefined
      ? "out of sight"
      : record.flags.dead ? `level ${record.level}, dead` : `level ${record.level}, ${record.healthPercent}%`;
    parts.push(`Target: ${name} (${detail})`);
  }
  if (viewer.autoAttacking) {
    parts.push("Attacking");
  }
  return parts.join(" · ");
}

/** One feedback line per event, from the viewer's point of view. */
export function eventText(event: ZoneEvent, snapshot: ZoneSnapshot, catalog: ContentCatalog): string {
  const name = (entity: EntityRef) => unitName(entity, snapshot, catalog);
  const critical = (isCritical: boolean) => (isCritical ? " (critical)" : "");
  switch (event.kind) {
    case "damage-dealt":
      return `You hit ${name(event.target)} for ${event.amount}${critical(event.critical)}.`;
    case "damage-taken":
      return `${capitalise(name(event.source))} hits you for ${event.amount}${critical(event.critical)}.`;
    case "miss":
      return event.source.kind === "player" && event.source.id === snapshot.viewerId
        ? `You miss ${name(event.target)}.`
        : `${capitalise(name(event.source))} misses you.`;
    case "evade":
      return `${capitalise(name(event.target))} evades.`;
    case "died":
      return event.entity.kind === "player" && event.entity.id === snapshot.viewerId
        ? "You die."
        : `${capitalise(name(event.entity))} dies.`;
    case "error":
      return ERROR_TEXT[event.code];
  }
}

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** How long a feedback line stays up, in milliseconds. */
export const FEEDBACK_MS = 3_000;

/** A HUD line the combat HUD writes; an `HTMLElement` in the page. */
export type HudLine = { textContent: string | null };

/**
 * The two combat HUD lines: the viewer's status, and the latest feedback
 * event, which fades after `FEEDBACK_MS`. Lines are written only when their
 * text changes.
 */
export class CombatHud {
  readonly #status: HudLine;
  readonly #feedback: HudLine;
  #latest: { text: string; until: number } | null = null;
  #tick = -1n;

  constructor(status: HudLine, feedback: HudLine) {
    this.#status = status;
    this.#feedback = feedback;
  }

  /** Clears both lines for a new session. */
  reset(): void {
    this.#latest = null;
    this.#tick = -1n;
    this.#status.textContent = "";
    this.#feedback.textContent = "";
  }

  /** Reads one projection; its events are read once, however many frames show it. */
  update(snapshot: ZoneSnapshot, catalog: ContentCatalog, now: number): void {
    write(this.#status, combatStatus(snapshot, catalog));
    if (snapshot.tick !== this.#tick) {
      this.#tick = snapshot.tick;
      const latest = snapshot.events.at(-1);
      if (latest) {
        this.#latest = { text: eventText(latest, snapshot, catalog), until: now + FEEDBACK_MS };
      }
    }
    write(this.#feedback, this.#latest && this.#latest.until > now ? this.#latest.text : "");
  }
}

function write(line: HudLine, text: string): void {
  if (line.textContent !== text) {
    line.textContent = text;
  }
}
