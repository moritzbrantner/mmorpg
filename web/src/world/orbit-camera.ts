import type { Axis } from "../command-wire";
import type { MovementInput } from "./movement-outbox";
import { yawFromRadians } from "../replication";

/**
 * Third-person orbit camera around the local character, mirroring the native
 * client's `OrbitCamera`. Presentation only: its heading becomes movement
 * intent while moving, never an authoritative position.
 */
const PIVOT_HEIGHT_METRES = 0.6;
export const MIN_DISTANCE_METRES = 2;
export const MAX_DISTANCE_METRES = 30;
const MIN_PITCH_RADIANS = 0.05;
const MAX_PITCH_RADIANS = 1.45;
export const ORBIT_RADIANS_PER_PIXEL = 0.006;
/** Each wheel line changes the distance by this factor. */
const ZOOM_STEP = 0.88;
/** Browser pixel scrolling: this many pixels count as one wheel line. */
export const PIXELS_PER_WHEEL_LINE = 40;

export type Vec3 = [number, number, number];
export type CameraView = { eye: Vec3; target: Vec3 };

export class OrbitCamera {
  /** World yaw the camera looks along: 0 toward +Z, increasing toward +X. */
  #yaw = 0;
  #pitch = 0.42;
  #distance = 9;

  get distance(): number {
    return this.#distance;
  }

  /** Mouse drag: moving right turns the view right; moving down raises the eye. */
  orbit(dxPixels: number, dyPixels: number): void {
    if (!Number.isFinite(dxPixels) || !Number.isFinite(dyPixels)) {
      return;
    }
    const turn = dxPixels * ORBIT_RADIANS_PER_PIXEL;
    const fullTurn = 2 * Math.PI;
    this.#yaw = (((this.#yaw - turn) % fullTurn) + fullTurn) % fullTurn;
    this.#pitch = Math.min(MAX_PITCH_RADIANS, Math.max(MIN_PITCH_RADIANS, this.#pitch + dyPixels * ORBIT_RADIANS_PER_PIXEL));
  }

  /** Positive wheel lines zoom in. */
  zoom(lines: number): void {
    if (Number.isFinite(lines)) {
      this.#distance = Math.min(MAX_DISTANCE_METRES, Math.max(MIN_DISTANCE_METRES, this.#distance * ZOOM_STEP ** lines));
    }
  }

  /** The heading a character takes while moving under this camera. */
  facing(): number {
    return yawFromRadians(this.#yaw);
  }

  /** Eye and look-at point for a focus at the character's body centre, in metres. */
  view(focus: Vec3): CameraView {
    const target: Vec3 = [focus[0], focus[1] + PIVOT_HEIGHT_METRES, focus[2]];
    const horizontal = this.#distance * Math.cos(this.#pitch);
    return {
      eye: [
        target[0] - Math.sin(this.#yaw) * horizontal,
        target[1] + this.#distance * Math.sin(this.#pitch),
        target[2] - Math.cos(this.#yaw) * horizontal,
      ],
      target,
    };
  }
}

const FORWARD_KEYS = ["KeyW", "ArrowUp"];
const BACKWARD_KEYS = ["KeyS", "ArrowDown"];
const LEFT_KEYS = ["KeyA", "KeyQ", "ArrowLeft"];
const RIGHT_KEYS = ["KeyD", "KeyE", "ArrowRight"];

export type HeldIntent = { forward: Axis; strafe: Axis; steering: boolean };

/** W/S run and backpedal, A/D (or Q/E) strafe; any held movement key steers by the camera. */
export function heldIntent(keys: ReadonlySet<string>): HeldIntent {
  const held = (codes: readonly string[]) => codes.some((code) => keys.has(code));
  const axis = (positive: boolean, negative: boolean): Axis => (positive === negative ? 0 : positive ? 1 : -1);
  const forward = held(FORWARD_KEYS);
  const backward = held(BACKWARD_KEYS);
  const left = held(LEFT_KEYS);
  const right = held(RIGHT_KEYS);
  return {
    forward: axis(forward, backward),
    strafe: axis(right, left),
    steering: forward || backward || left || right,
  };
}

/**
 * Movement intent from the held keys under the orbit camera. While any
 * movement key is held the character faces the camera's heading; otherwise
 * it keeps `lastFacing`, so orbiting while idle never turns it.
 */
export function movementInput(
  keys: ReadonlySet<string>,
  cameraFacing: number,
  lastFacing: number,
  jumps: number,
): MovementInput {
  const held = heldIntent(keys);
  return { forward: held.forward, strafe: held.strafe, facing: held.steering ? cameraFacing : lastFacing, jumps };
}
