import * as THREE from "three";
import type {
  Matrix4Values,
  RendererCamera,
  RendererSceneNode,
  RendererWorkObservations,
  ThreeSceneRenderer,
} from "@moritzbrantner/three-d-renderer";
import type { WorldCommand } from "../command-wire";
import { entityKey } from "../entity-ref";
import type { EntityRef } from "../entity-ref";
import type { EntityState, ZoneSnapshot } from "../replication";
import { DebugOverlay, FrameRate } from "./debug-overlay";
import { ENVIRONMENT } from "./environment";
import type { LocalWorld } from "./local-world";
import { Minimap, buildMinimapLayers, dispositionOf, type MinimapElements, type MinimapUnit } from "./minimap";
import { MovementOutbox, type MovementInput } from "./movement-outbox";
import {
  IDLE_INTENT,
  OrbitCamera,
  PIXELS_PER_WHEEL_LINE,
  movementInput,
  type DragMode,
  type HeldIntent,
  type Vec3,
} from "./orbit-camera";
import { SceneryFrame, buildSceneryScene, sceneryResourcePrefix, type SceneryScene } from "./scenery-nodes";
import { SkyLayer } from "./sky";
import { UnitAnimators, placeWithModel, unitIdentity, unitModel, unitNodeIds, type UnitContext, type UnitLook } from "./unit-nodes";
import { BagsPanel, type BagsElements } from "./units/bags-panel";
import { useAbilitySlot } from "./units/abilities";
import { ClassHud } from "./units/class-hud";
import { CombatHud } from "./units/combat-hud";
import { LootPanel, type LootElements } from "./units/loot-panel";
import { nearestAnimal, projectedUnit, type ProjectedUnit } from "./units/projected-units";
import { ProgressionHud } from "./units/progression-hud";
import { SpellEffects, allEffectNodes, type Anchor } from "./units/spell-effects";
import { SecondaryClick, attackToggle } from "./units/targeting";
import type { ContentCatalog } from "./catalog";

/**
 * The in-world presentation: static scenery, animated units, the orbit
 * camera and its input, the CSS sky, the minimap and the F3 overlay. It reads
 * decoded player-scoped projections from the world source and sends intent
 * through the movement outbox; it owns no gameplay rules.
 */
export type WorldViewElements = {
  canvas: HTMLCanvasElement;
  sky: HTMLElement;
  minimap: MinimapElements;
  overlay: HTMLElement;
  areaName: HTMLElement;
  objective: HTMLElement;
  /** The viewer's health and target line. */
  unitStatus: HTMLElement;
  /** The latest combat feedback line. */
  combatFeedback: HTMLElement;
  experienceBar: HTMLProgressElement;
  experienceStatus: HTMLElement;
  progressionFeedback: HTMLElement;
  bags: BagsElements;
  loot: LootElements;
  /** Action bar, unit frames, cast bars and combat text. */
  classHud: HTMLElement;
};

/** A targeting or attack intent, resolved against the latest projection when it is sent. */
export type Intent = (projection: ZoneSnapshot) => WorldCommand | null;

/** Outside every named area the HUD names the zone itself. */
const ZONE_NAME = "Greyhaven Vale";

/** Debug-only camera poses for screenshots; they never touch the simulation. */
export const DEBUG_VIEWPOINTS: Readonly<Record<string, { eye: Vec3; target: Vec3 }>> = {
  hub: { eye: [5, 8.5, 31], target: [-14, 3, 4] },
  grass: { eye: [-23, 16, 29], target: [-23, 0, 11] },
  relief: { eye: [20, 8, 39], target: [0, 0, 18] },
  woods: { eye: [-46, 7, 14], target: [-78, 3, -2] },
  hollow: { eye: [6, 14, -64], target: [0, 3, -98] },
  farm: { eye: [46, 12, 16], target: [72, 3, 40] },
  lake: { eye: [30, 9, -34], target: [56, 0, -56] },
  vale: { eye: [0, 70, 95], target: [0, 0, -10] },
};

export function webGpuProjection(source: THREE.PerspectiveCamera): Matrix4Values {
  const gl = source.projectionMatrix.elements;
  const gpu = [...gl];
  for (const index of [2, 6, 10, 14]) {
    const z = gl[index];
    const w = gl[index + 1];
    if (z === undefined || w === undefined) {
      throw new Error("Incomplete projection matrix");
    }
    gpu[index] = (z + w) / 2;
  }
  return gpu as Matrix4Values;
}

type Drag = { pointerId: number; x: number; y: number; mode: Exclude<DragMode, "none"> };
type Box = { min: Vec3; max: Vec3 };

/** Structure collider boxes in metres, for choosing an unobstructed first view. */
function occluders(world: LocalWorld): Box[] {
  const { props, unitsPerMetre } = world.scenery.scenery;
  const margin = 0.4;
  return props.flatMap((prop) => {
    const collider = prop.collider;
    if (!collider) {
      return [];
    }
    const [cx, cy, cz] = collider.center.map((value) => value / unitsPerMetre) as Vec3;
    const [hx, hy, hz] = collider.halfExtents.map((value) => value / unitsPerMetre) as Vec3;
    return [{ min: [cx - hx - margin, cy - hy, cz - hz - margin], max: [cx + hx + margin, cy + hy + margin, cz + hz + margin] }];
  });
}

/** Whether the sight line from `target` to `eye` passes through a box (sampled). */
function obstructed(boxes: readonly Box[], target: Vec3, eye: Vec3): boolean {
  for (const t of [0.35, 0.6, 0.8, 1]) {
    const point = [0, 1, 2].map((axis) => target[axis]! + (eye[axis]! - target[axis]!) * t) as Vec3;
    if (boxes.some((box) => point.every((value, axis) => value >= box.min[axis]! && value <= box.max[axis]!))) {
      return true;
    }
  }
  return false;
}

export type WorldInput = { held: HeldIntent; jumps: number };

export class WorldView {
  readonly #renderer: ThreeSceneRenderer;
  readonly #camera: THREE.PerspectiveCamera;
  readonly #elements: WorldViewElements;
  readonly #reducedMotion: MediaQueryList;
  readonly #overlay: DebugOverlay;
  readonly #frameRate = new FrameRate();
  readonly #sky: SkyLayer;
  readonly #animators = new UnitAnimators();
  readonly #combatHud: CombatHud;
  readonly #progressionHud: ProgressionHud;
  readonly #classHud: ClassHud;
  readonly #effects = new SpellEffects();
  readonly #bags: BagsPanel;
  readonly #loot: LootPanel;
  readonly #intents: Intent[] = [];
  readonly #secondaryClick = new SecondaryClick();
  #scene: SceneryScene | null = null;
  #catalog: ContentCatalog | null = null;
  /** Where each visible unit stood in the last frame (metres), for spell effects. */
  #anchors = new Map<string, Anchor>();
  /** The unit nodes of the last frame, for debug readouts. */
  #lastUnitNodes: readonly RendererSceneNode[] = [];
  /** The projected units of the last frame, for debug readouts. */
  #lastProjectedUnits: readonly ProjectedUnit[] = [];
  #sceneryFrame: SceneryFrame | null = null;
  #presentationFingerprint: string | null = null;
  #reliefAt: ((x: number, z: number) => number) | null = null;
  #minimap: Minimap | null = null;
  #buildMs = 0;
  #orbit = new OrbitCamera();
  #drag: Drag | null = null;
  #movementFacing = 0;
  #outbox: MovementOutbox;
  #shownArea: string | null = null;
  #seconds = 0;
  #flyTo: { eye: Vec3; target: Vec3 } | null = null;
  #lastWork: RendererWorkObservations | null = null;
  #occluders: Box[] = [];
  /** The viewer's projected unit as last drawn, for debug readouts. */
  #lastSelf: { x: number; z: number; facing: number } | null = null;
  /** The first frame of a session picks a heading whose view is not inside a building. */
  #framePending = false;

  constructor(renderer: ThreeSceneRenderer, camera: THREE.PerspectiveCamera, elements: WorldViewElements) {
    this.#renderer = renderer;
    this.#camera = camera;
    this.#elements = elements;
    this.#reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    this.#overlay = new DebugOverlay(elements.overlay);
    this.#sky = new SkyLayer(elements.sky, ENVIRONMENT);
    this.#combatHud = new CombatHud(elements.unitStatus, elements.combatFeedback);
    this.#classHud = new ClassHud(elements.classHud, {
      onUse: (slot) => this.queueIntent((projection) => (this.#catalog ? useAbilitySlot(slot, projection, this.#catalog) : null)),
    });
    this.#progressionHud = new ProgressionHud(elements.experienceBar, elements.experienceStatus, elements.progressionFeedback);
    this.#bags = new BagsPanel(elements.bags, (command) => this.queueIntent(() => command));
    this.#loot = new LootPanel(elements.loot, (intent) => this.queueIntent(intent), () => { this.#bags.close(); });
    elements.bags.toggle.addEventListener("click", () => this.#loot.close(false));
    this.#outbox = new MovementOutbox(this.#input({ held: IDLE_INTENT, jumps: 0 }));
  }

  get #animate(): boolean {
    return !this.#reducedMotion.matches;
  }

  /** Builds the static scene and the minimap image once per loaded world. */
  load(world: LocalWorld): void {
    const started = performance.now();
    this.#catalog = world.catalog;
    this.#bags.load(world.catalog);
    this.#loot.load(world.catalog);
    const scenery = world.scenery.scenery;
    this.#presentationFingerprint = scenery.presentationFingerprint;
    this.#reliefAt = world.scenery.reliefAt;
    this.#scene = buildSceneryScene(scenery, ENVIRONMENT);
    this.#sceneryFrame = new SceneryFrame(this.#scene, `${sceneryResourcePrefix(scenery)}:animated`);
    this.#minimap = new Minimap(this.#elements.minimap, buildMinimapLayers(scenery, ENVIRONMENT));
    this.#occluders = occluders(world);
    this.#buildMs = performance.now() - started;
  }

  /** Starts a session: fresh camera, intent and animation state. */
  enter(input: WorldInput, projection: ZoneSnapshot | null): void {
    this.#orbit = new OrbitCamera();
    this.#movementFacing = this.#orbit.facing();
    this.#endDrag();
    this.#outbox.reset(this.#input(input));
    this.#animators.clear();
    this.#intents.length = 0;
    this.#combatHud.reset();
    this.#progressionHud.reset();
    this.#classHud.reset();
    this.#effects.clear();
    this.#anchors = new Map();
    this.#bags.reset(projection);
    this.#loot.reset(projection);
    this.#shownArea = null;
    this.#flyTo = null;
    this.#framePending = true;
    this.#frameRate.reset();
    this.#sky.show(true);
  }

  /**
   * Starts behind the character, or turned in eighth turns until the sight
   * line clears every structure. The camera itself never collides; this only
   * frames the first view of a session, deterministically.
   */
  #frameFirstView(focus: Vec3, facing: number, groundAt: (x: number, z: number) => number): void {
    for (const step of [0, 2, -2, 1, -1, 3, -3, 4]) {
      this.#orbit.lookAlong(facing + (step * Math.PI) / 4);
      const view = this.#orbit.view(focus, groundAt);
      if (!obstructed(this.#occluders, view.target, view.eye)) {
        return;
      }
    }
    this.#orbit.lookAlong(facing);
  }

  leave(): void {
    this.#classHud.reset();
    this.#effects.clear();
    this.#bags.reset();
    this.#loot.reset();
    this.#endDrag();
    this.#sky.show(false);
    this.#overlay.hide();
  }

  /** Queues an intent for the next frame; the zone decides what happens. */
  queueIntent(intent: Intent): void {
    this.#intents.push(intent);
  }

  toggleBags(): void {
    this.#loot.close(false);
    this.#bags.toggle();
  }

  closeLoot(): boolean { return this.#loot.close(); }

  closeBags(): boolean { return this.#bags.close(); }

  /** Whether bags or loot are open: a non-blocking overlay over gameplay controls. */
  get panelOpen(): boolean { return this.#bags.open || this.#loot.open; }

  toggleOverlay(): void {
    this.#overlay.toggle();
  }

  #input({ held, jumps }: WorldInput): MovementInput {
    const input = movementInput(held, this.#orbit.facing(), this.#movementFacing, jumps, this.#drag?.mode ?? "none");
    this.#movementFacing = input.facing;
    return input;
  }

  #showArea(name: string): void {
    if (name === this.#shownArea) {
      return;
    }
    this.#shownArea = name;
    this.#elements.areaName.textContent = name;
    // A placeholder objective until quests exist; area names come from core content.
    this.#elements.objective.textContent = name === ZONE_NAME
      ? "Explore Greyhaven Vale. Quests are not in this build yet."
      : `Explore Greyhaven Vale — you are in ${name}. Quests are not in this build yet.`;
  }

  /** One frame: intent out, ticks, projection in, scene drawn. Throws when the projection lacks the viewer. */
  frame(world: LocalWorld, input: WorldInput, look: UnitLook, deltaSeconds: number, now: number): void {
    const scene = this.#scene;
    const sceneryFrame = this.#sceneryFrame;
    if (!scene || !sceneryFrame) {
      throw new Error("The world view has no scenery loaded.");
    }
    const { source, scenery, catalog } = world;
    const latest = source.latestProjection();
    for (const intent of this.#intents.splice(0)) {
      const command = latest ? intent(latest) : null;
      if (command) {
        source.sendCommand(command);
      }
    }
    for (const command of this.#outbox.update(this.#input(input), now)) {
      source.sendCommand(command);
    }
    const receivedProjections = source.advance(deltaSeconds);
    for (const received of receivedProjections) {
      this.#bags.update(received);
      this.#loot.update(received);
    }
    const projection = source.latestProjection();
    if (!projection) {
      throw new Error("The local zone has no projection for the joined player.");
    }
    const animate = this.#animate;
    this.#seconds += deltaSeconds;
    const resolveAnchor = (entity: EntityRef): Anchor | null => this.#anchors.get(entityKey(entity)) ?? null;
    for (const received of receivedProjections) {
      this.#classHud.receive(received, now);
      this.#effects.spawn(received, this.#seconds, resolveAnchor);
    }
    this.#effects.sync(projection, this.#seconds, resolveAnchor);
    this.#orbit.update(deltaSeconds, !animate);
    const { unitsPerMetre, playerHalfExtents } = scenery.scenery;
    const unitContext: UnitContext = {
      unitsPerMetre,
      playerHalfHeightUnits: playerHalfExtents[1],
      viewerId: projection.viewerId,
      viewerLook: look,
      catalog,
      viewerTarget: projection.viewer.target,
      viewerAction: this.#effects.viewerAction(projection, this.#seconds),
    };
    const nodes: RendererSceneNode[] = [];
    const units: RendererSceneNode[] = [];
    const visible = new Set<string>();
    const projected: ProjectedUnit[] = [];
    const others: MinimapUnit[] = [];
    const anchors = new Map<string, Anchor>();
    let self: { focus: Vec3; x: number; z: number; facing: number } | null = null;
    for (const entity of source.sample()) {
      const model = unitModel(entity);
      const placement = placeWithModel(model, entity, unitContext, scenery.reliefAt(entity.position[0], entity.position[2]));
      const isSelf = entity.kind === "player" && entity.entityId === projection.viewerId;
      const id = unitIdentity(entity);
      visible.add(id);
      projected.push(projectedUnit(entity, placement, catalog));
      const locomotion = this.#animators.locomotion(entity, placement, unitsPerMetre, deltaSeconds, !animate);
      units.push(...model.nodes({ id, entity, placement, locomotion, context: unitContext }));
      anchors.set(entityKey({ kind: entity.kind, id: entity.entityId }), {
        x: placement.x,
        feetY: placement.feetY,
        centreY: placement.feetY + model.halfHeightUnits(entity, unitContext) / unitsPerMetre,
        z: placement.z,
        yaw: placement.yawRadians,
      });
      if (isSelf) {
        const centreY = placement.feetY + model.halfHeightUnits(entity, unitContext) / unitsPerMetre;
        self = { focus: [placement.x, centreY, placement.z], x: placement.x, z: placement.z, facing: placement.yawRadians };
        this.#showArea(scenery.areaAt(entity.position[0], entity.position[2])?.name ?? ZONE_NAME);
      } else {
        others.push(this.#minimapUnit(entity, unitsPerMetre));
      }
    }
    this.#animators.retain(visible);
    this.#anchors = anchors;
    if (!self) {
      throw new Error("The projection is missing the viewer's own unit.");
    }
    this.#combatHud.update(projection, catalog, now);
    this.#classHud.update(projection, catalog, now);
    this.#progressionHud.update(projection, now);
    this.#bags.update(projection);
    this.#loot.update(projection);
    this.#lastSelf = { x: self.x, z: self.z, facing: self.facing };
    if (this.#framePending) {
      this.#framePending = false;
      this.#frameFirstView(self.focus, self.facing, scene.surfaceY);
    }
    const view = this.#flyTo ?? this.#orbit.view(self.focus, scene.surfaceY);
    this.#camera.position.set(...view.eye);
    this.#camera.lookAt(...view.target);
    this.#camera.updateMatrixWorld(true);
    const frame = sceneryFrame.nodes(view.eye, { seconds: this.#seconds, animate });
    nodes.push(...frame.nodes, ...units, ...allEffectNodes(this.#effects.active(this.#seconds), this.#seconds, resolveAnchor));
    this.#lastUnitNodes = units;
    this.#lastProjectedUnits = projected;
    const camera: RendererCamera = {
      viewMatrix: [...this.#camera.matrixWorldInverse.elements] as Matrix4Values,
      projectionMatrix: webGpuProjection(this.#camera),
    };
    this.#lastWork = this.#renderer.render({ camera, nodes });
    this.#sky.update(this.#horizon(view.eye, view.target), this.#seconds, animate);
    this.#minimap?.draw({ x: self.x, z: self.z }, this.#orbit.heading, self.facing, others);
    this.#frameRate.sample(now);
    this.#overlay.update({
      fps: this.#frameRate.fps,
      frameMs: this.#frameRate.frameMs,
      nodes: nodes.length,
      staticNodes: scene.stats.staticNodes,
      visibleStaticNodes: frame.visibleBatches,
      staticVertices: scene.stats.staticVertices,
      units: visible.size,
      work: this.#lastWork,
    }, now);
  }

  #minimapUnit(entity: EntityState, unitsPerMetre: number): MinimapUnit {
    return { x: entity.position[0] / unitsPerMetre, z: entity.position[2] / unitsPerMetre, disposition: dispositionOf(entity) };
  }

  /** The horizon's height on screen as a fraction from the top, for the CSS sky. */
  #horizon(eye: Vec3, target: Vec3): number {
    const dx = target[0] - eye[0];
    const dz = target[2] - eye[2];
    const length = Math.hypot(dx, dz) || 1;
    const far = new THREE.Vector3(eye[0] + (dx / length) * 5_000, eye[1], eye[2] + (dz / length) * 5_000);
    far.project(this.#camera);
    return (1 - far.y) / 2;
  }

  pointerDown(event: PointerEvent): void {
    if (this.#drag || (event.button !== 0 && event.button !== 2)) {
      return;
    }
    try {
      this.#elements.canvas.setPointerCapture(event.pointerId);
    } catch {
      return;
    }
    const mode = event.button === 2 ? "turn" : "orbit";
    this.#drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, mode };
    this.#secondaryClick.down(event);
    this.#elements.canvas.dataset.drag = mode;
  }

  pointerMove(event: PointerEvent): void {
    this.#secondaryClick.move(event);
    const drag = this.#drag;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }
    this.#orbit.orbit(event.clientX - drag.x, event.clientY - drag.y);
    drag.x = event.clientX;
    drag.y = event.clientY;
  }

  pointerEnd(event: PointerEvent): void {
    // A right click that never became a turn drag toggles auto-attack.
    if (this.#secondaryClick.end(event)) {
      this.queueIntent(attackToggle);
    }
    if (this.#drag?.pointerId === event.pointerId) {
      this.#endDrag();
    }
  }

  #endDrag(): void {
    this.#secondaryClick.reset();
    const drag = this.#drag;
    if (drag && this.#elements.canvas.hasPointerCapture(drag.pointerId)) {
      this.#elements.canvas.releasePointerCapture(drag.pointerId);
    }
    this.#drag = null;
    delete this.#elements.canvas.dataset.drag;
  }

  wheel(event: WheelEvent): void {
    const lines = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL
      ? event.deltaY / PIXELS_PER_WHEEL_LINE
      : event.deltaMode === WheelEvent.DOM_DELTA_LINE ? event.deltaY : event.deltaY * 3;
    // Scrolling up (negative delta) zooms in.
    this.#orbit.zoom(-lines);
  }

  /** Debug-only hooks for screenshots and measurements; the simulation never sees them. */
  debugApi() {
    return {
      viewpoints: Object.keys(DEBUG_VIEWPOINTS),
      flyTo: (name: string) => {
        const viewpoint = DEBUG_VIEWPOINTS[name];
        if (!viewpoint) {
          throw new Error(`Unknown viewpoint ${name}.`);
        }
        this.#flyTo = viewpoint;
      },
      flyToPose: (eye: Vec3, target: Vec3) => {
        this.#flyTo = { eye, target };
      },
      follow: () => {
        this.#flyTo = null;
      },
      /** Turns the view (and so forward movement) toward a point in metres. */
      faceToward: (x: number, z: number) => {
        const from = this.#lastSelf;
        if (from) {
          this.#orbit.lookAlong(Math.atan2(x - from.x, z - from.z));
        }
      },
      reliefAt: (x: number, z: number) => {
        if (!this.#reliefAt) {
          throw new Error("Scenery is not loaded");
        }
        return this.#reliefAt(x, z);
      },
      /** Node IDs the last frame drew for one unit, e.g. `unit-creature-108`. */
      unitNodeIds: (identity: string) => unitNodeIds(this.#lastUnitNodes, identity),
      /** The units of the last projection (kind, id, creature family, position in metres). */
      projectedUnits: () => this.#lastProjectedUnits,
      /** The projected animal (wolf, boar, vermin) nearest to a point in metres, or null. */
      nearestAnimal: (x: number, z: number) => nearestAnimal(this.#lastProjectedUnits, x, z),
      stats: () => ({
        presentationFingerprint: this.#presentationFingerprint,
        buildMs: Number(this.#buildMs.toFixed(1)),
        scene: this.#scene?.stats ?? null,
        frame: this.#overlay.latest,
        self: this.#lastSelf,
      }),
    };
  }
}
