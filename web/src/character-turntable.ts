/** Presentation only: rotating the preview must never change gameplay facing or saves. */
export const PREVIEW_FRONT_YAW = Math.atan2(-4.5, -7.1);
export const TURN_STEP_DEGREES = 15;

export function wrapDegrees(degrees: number): number {
  if (!Number.isFinite(degrees)) throw new Error("Preview rotation must be finite.");
  const wrapped = ((degrees % 360) + 360) % 360;
  return Object.is(wrapped, -0) ? 0 : wrapped;
}

export class CharacterTurntable {
  #degrees = 0;
  #drag: { pointerId: number; startX: number; width: number; startDegrees: number } | null = null;

  get degrees(): number { return this.#degrees; }
  get yaw(): number { return PREVIEW_FRONT_YAW + this.#degrees * Math.PI / 180; }
  get pointerId(): number | null { return this.#drag?.pointerId ?? null; }

  turn(degrees: number): void {
    this.cancel();
    this.#degrees = wrapDegrees(this.#degrees + degrees);
  }

  reset(): void {
    this.cancel();
    this.#degrees = 0;
  }

  begin(pointerId: number, x: number, width: number): boolean {
    if (this.#drag || !Number.isInteger(pointerId) || !Number.isFinite(x) || !Number.isFinite(width) || width <= 0) return false;
    this.#drag = { pointerId, startX: x, width, startDegrees: this.#degrees };
    return true;
  }

  move(pointerId: number, x: number): boolean {
    const drag = this.#drag;
    if (!drag || drag.pointerId !== pointerId || !Number.isFinite(x)) return false;
    // One surface width is one revolution; use the original anchor to avoid drift.
    const next = drag.startDegrees + (x - drag.startX) / drag.width * 360;
    if (!Number.isFinite(next)) return false;
    this.#degrees = wrapDegrees(next);
    return true;
  }

  end(pointerId: number): boolean {
    if (this.#drag?.pointerId !== pointerId) return false;
    this.#drag = null;
    return true;
  }

  cancel(pointerId = this.#drag?.pointerId): boolean {
    if (!this.#drag || this.#drag.pointerId !== pointerId) return false;
    this.#degrees = this.#drag.startDegrees;
    this.#drag = null;
    return true;
  }
}

/** Pointer capture is confined to the preview surface, never the equipment/save panels. */
export function installCharacterTurntable(
  surface: HTMLElement,
  buttons: { left: HTMLButtonElement; right: HTMLButtonElement; reset: HTMLButtonElement },
  isActive: () => boolean,
) {
  const model = new CharacterTurntable();
  const refresh = () => {
    const degrees = Math.round(model.degrees) % 360;
    surface.setAttribute("aria-valuenow", String(degrees));
    surface.setAttribute("aria-valuetext", `${degrees} degrees from front`);
    surface.dataset.dragging = String(model.pointerId !== null);
  };
  const release = (pointerId: number | null) => {
    if (pointerId !== null && surface.hasPointerCapture(pointerId)) surface.releasePointerCapture(pointerId);
  };
  const cancel = () => {
    const pointerId = model.pointerId;
    model.cancel();
    release(pointerId);
    refresh();
  };
  const turn = (degrees: number) => {
    if (!isActive()) return;
    cancel();
    model.turn(degrees);
    refresh();
  };
  const reset = () => {
    if (!isActive()) return;
    cancel();
    model.reset();
    refresh();
  };
  buttons.left.addEventListener("click", () => turn(-TURN_STEP_DEGREES));
  buttons.right.addEventListener("click", () => turn(TURN_STEP_DEGREES));
  buttons.reset.addEventListener("click", reset);
  surface.addEventListener("keydown", (event) => {
    if (!isActive() || event.altKey || event.ctrlKey || event.metaKey) return;
    switch (event.key) {
      case "ArrowLeft": case "ArrowDown": turn(-TURN_STEP_DEGREES); break;
      case "ArrowRight": case "ArrowUp": turn(TURN_STEP_DEGREES); break;
      case "Home": reset(); break;
      case "End": reset(); turn(359); break;
      case "Escape": cancel(); break;
      default: return;
    }
    event.preventDefault();
    event.stopPropagation();
  });
  surface.addEventListener("pointerdown", (event) => {
    if (!isActive() || !event.isPrimary || event.button !== 0) return;
    if (!model.begin(event.pointerId, event.clientX, surface.getBoundingClientRect().width)) return;
    try {
      surface.setPointerCapture(event.pointerId);
    } catch {
      cancel();
      return;
    }
    surface.focus({ preventScroll: true });
    refresh();
  });
  surface.addEventListener("pointermove", (event) => {
    if (model.pointerId !== event.pointerId) return;
    if (!isActive() || (event.pointerType !== "touch" && (event.buttons & 1) === 0)) {
      cancel();
      return;
    }
    if (model.move(event.pointerId, event.clientX)) refresh();
  });
  surface.addEventListener("pointerup", (event) => {
    if (!isActive()) { cancel(); return; }
    if (!model.end(event.pointerId)) return;
    release(event.pointerId);
    refresh();
  });
  for (const name of ["pointercancel", "lostpointercapture"] as const) {
    surface.addEventListener(name, (event) => {
      if (model.pointerId === event.pointerId) cancel();
    });
  }
  surface.addEventListener("blur", cancel);
  window.addEventListener("blur", cancel);
  document.addEventListener("visibilitychange", () => { if (document.hidden) cancel(); });
  refresh();
  return { get yaw() { return model.yaw; }, cancel };
}
