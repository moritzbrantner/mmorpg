import type { EntityRef } from "../../entity-ref";
import type { ZoneEvent, ZoneSnapshot } from "../../replication";

/**
 * Floating combat text for the viewer's projection events. Events are cosmetic and lossy, so the
 * text only decorates what the self and target sections already show.
 */
export type CombatTextKind = "damage" | "critical" | "periodic" | "taken" | "heal" | "absorb" | "interrupt" | "miss";

export type CombatText = { kind: CombatTextKind; text: string };

function isViewer(entity: EntityRef | null, snapshot: ZoneSnapshot): boolean {
  return entity !== null && entity.kind === "player" && entity.id === snapshot.viewerId;
}

/**
 * Whether the viewer's damage in this projection came from a damage-over-time tick or a channel
 * pulse rather than a swing or a spell: no ability resolved this tick, no auto-attack runs, and
 * either a channel is running or the target carries a damage-over-time aura.
 */
export function damageIsPeriodic(snapshot: ZoneSnapshot): boolean {
  if (snapshot.viewer.cast?.channel) {
    return true;
  }
  const resolved = snapshot.events.some((event) => event.kind === "ability-used" && isViewer(event.source, snapshot));
  return !resolved
    && !snapshot.viewer.autoAttacking
    && snapshot.targetDetail.auras.some((aura) => aura.kind === "damage-over-time");
}

function textFor(event: ZoneEvent, snapshot: ZoneSnapshot, periodic: boolean): CombatText | null {
  switch (event.kind) {
    case "damage-dealt":
      return {
        kind: event.critical ? "critical" : periodic ? "periodic" : "damage",
        text: event.critical ? `${event.amount}!` : `${event.amount}`,
      };
    case "damage-taken":
      return { kind: "taken", text: `-${event.amount}${event.critical ? "!" : ""}` };
    case "miss":
      return isViewer(event.source, snapshot) ? { kind: "miss", text: "Miss" } : null;
    case "evade":
      return { kind: "miss", text: "Evade" };
    case "healed":
      return isViewer(event.target, snapshot) || isViewer(event.source, snapshot)
        ? { kind: "heal", text: `+${event.amount}` }
        : null;
    case "absorbed":
      return { kind: "absorb", text: "Absorb" };
    case "interrupted":
      return { kind: "interrupt", text: isViewer(event.target, snapshot) ? "Interrupted" : "Interrupt" };
    default:
      return null;
  }
}

/** The floating texts one projection's events produce, in event order. */
export function combatTexts(snapshot: ZoneSnapshot): readonly CombatText[] {
  const periodic = damageIsPeriodic(snapshot);
  return snapshot.events.flatMap((event) => {
    const text = textFor(event, snapshot, periodic);
    return text ? [text] : [];
  });
}

/** How long a floating text lives, in milliseconds. */
export const COMBAT_TEXT_MS = 1_400;

/** The interrupt flash on the player cast bar shows for this long. */
export const INTERRUPT_FLASH_MS = 900;

/** Whether this projection interrupted the viewer's own cast. */
export function viewerInterrupted(snapshot: ZoneSnapshot): boolean {
  return snapshot.events.some((event) => event.kind === "interrupted" && isViewer(event.target, snapshot));
}
