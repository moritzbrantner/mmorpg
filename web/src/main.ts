import * as THREE from "three";
import {
  createThreeSceneRenderer,
  type Matrix4Values,
  type RendererSceneNode,
} from "@moritzbrantner/three-d-renderer";
import "./styles.css";
import { SnapshotBuffer, TICK_HZ, UNITS_PER_METRE, type Vector3 } from "./replication";

function requireElement<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`Tech demo shell is missing ${selector}`);
  }
  return element;
}

const canvas = requireElement<HTMLCanvasElement>("#world");
const prompt = requireElement<HTMLElement>("#prompt");
const objective = requireElement<HTMLElement>("#objective");
const status = requireElement<HTMLElement>("#status");

const renderer = createThreeSceneRenderer(canvas, {
  background: "#9fb1a1",
  antialias: true,
  pixelRatioLimit: 2,
});

type Vec2 = { x: number; z: number };
const player: Vec2 = { x: -5.5, z: 4.5 };
let facing = Math.PI * 0.15;
let waystoneActive = false;
let lastTime = performance.now();
const keys = new Set<string>();
// Offline demo source. An online source supplies decoded player-scoped snapshots
// to the same presentation buffer and sends input commands to the zone host.
const snapshots = new SnapshotBuffer();
let demoTick = 0n;
let accumulatedTicks = 0;

function publishDemoSnapshot() {
  snapshots.push({
    zoneId: 1,
    tick: demoTick,
    contentRevision: 0n,
    acknowledgedSequence: 0,
    players: [{
      playerId: 1,
      position: [Math.round(player.x * UNITS_PER_METRE), 83, Math.round(player.z * UNITS_PER_METRE)],
      velocity: [0, 0, 0],
    }],
  });
}

publishDemoSnapshot();

const worldHalfExtent = 10.5;
const waystone = { x: 4.8, z: -3.5 };
const interactRadius = 2.1;

const camera = new THREE.PerspectiveCamera(48, 1, 0.1, 80);
const target = new THREE.Vector3();

const staticNodes: RendererSceneNode[] = [
  node("ground", "box", [0, -0.3, 0], "#6d7d62", [24, 0.6, 24]),
  node("road-a", "box", [0, 0.02, 0], "#8d806a", [4.2, 0.08, 22]),
  node("road-b", "box", [2.8, 0.03, -3.2], "#8d806a", [9.5, 0.09, 3.2]),
  node("hut-1", "box", [-6.8, 1.2, -4.8], "#775b45", [3.6, 2.4, 3.4]),
  node("hut-2", "box", [6.8, 1.0, 4.7], "#6f5845", [3.2, 2.0, 3.8]),
  node("crate-1", "box", [-2.5, 0.55, -3.9], "#8a6444", [1.1, 1.1, 1.1]),
  node("crate-2", "box", [-1.2, 0.45, -4.2], "#75543d", [0.9, 0.9, 0.9]),
  node("tree-1-trunk", "cylinder", [-8.3, 1.25, 2.8], "#5f4634", [0.35, 2.5, 0]),
  node("tree-1-crown", "sphere", [-8.3, 3.25, 2.8], "#365e3d", [1.55, 0, 0]),
  node("tree-2-trunk", "cylinder", [8.0, 1.2, -7.3], "#5f4634", [0.32, 2.4, 0]),
  node("tree-2-crown", "sphere", [8.0, 3.05, -7.3], "#31583a", [1.4, 0, 0]),
  node("stone-a", "box", [5.9, 0.3, -4.1], "#777e78", [1.4, 0.6, 0.7]),
  node("stone-b", "box", [4.2, 0.25, -5.0], "#727972", [0.8, 0.5, 1.1]),
];

function node(
  id: string,
  kind: "box" | "sphere" | "cylinder",
  translation: [number, number, number],
  color: `#${string}`,
  dimensions: [number, number, number],
): RendererSceneNode {
  const geometry =
    kind === "box"
      ? { kind, size: dimensions }
      : kind === "sphere"
        ? { kind, radius: dimensions[0] }
        : { kind, radius: dimensions[0], height: dimensions[1] };

  return { id, geometry, color, transform: { translation } };
}

function webGpuProjectionFromThree(): Matrix4Values {
  const gl = camera.projectionMatrix.elements;
  const gpu = [...gl];
  for (const index of [2, 6, 10, 14]) {
    const z = gl[index];
    const w = gl[index + 1];
    if (z === undefined || w === undefined) throw new Error("Incomplete projection matrix");
    gpu[index] = (z + w) / 2;
  }
  return gpu as Matrix4Values;
}

function updateMovement(deltaSeconds: number) {
  let dx = 0;
  let dz = 0;
  if (keys.has("KeyW") || keys.has("ArrowUp")) dz -= 1;
  if (keys.has("KeyS") || keys.has("ArrowDown")) dz += 1;
  if (keys.has("KeyA") || keys.has("ArrowLeft")) dx -= 1;
  if (keys.has("KeyD") || keys.has("ArrowRight")) dx += 1;
  if (dx === 0 && dz === 0) return;

  const length = Math.hypot(dx, dz);
  dx /= length;
  dz /= length;
  const speed = keys.has("ShiftLeft") || keys.has("ShiftRight") ? 6.3 : 3.8;
  player.x = THREE.MathUtils.clamp(player.x + dx * speed * deltaSeconds, -worldHalfExtent, worldHalfExtent);
  player.z = THREE.MathUtils.clamp(player.z + dz * speed * deltaSeconds, -worldHalfExtent, worldHalfExtent);
  facing = Math.atan2(dx, dz);
}

function nearWaystone() {
  return Math.hypot(player.x - waystone.x, player.z - waystone.z) <= interactRadius;
}

function interact() {
  if (!nearWaystone()) return;
  waystoneActive = !waystoneActive;
  status.textContent = waystoneActive ? "Waystone active" : "Exploring";
  objective.textContent = waystoneActive
    ? "Waystone activated. Explore the outpost."
    : "Reach the old waystone and activate it.";
}

function dynamicNodes(position: Vector3): RendererSceneNode[] {
  const x = position[0] / UNITS_PER_METRE;
  const y = position[1] / UNITS_PER_METRE;
  const z = position[2] / UNITS_PER_METRE;
  const halfYaw = facing / 2;
  return [
    {
      id: "player",
      geometry: { kind: "cylinder", radius: 0.42, height: 1.65 },
      color: "#d8d2bd",
      transform: {
        translation: [x, y, z],
        rotationQuaternion: [0, Math.sin(halfYaw), 0, Math.cos(halfYaw)],
      },
    },
    {
      id: "player-head",
      geometry: { kind: "sphere", radius: 0.34 },
      color: "#c49b78",
      transform: { translation: [x, y + 1.07, z] },
    },
    {
      id: "waystone",
      geometry: { kind: "box", size: [0.9, 2.7, 0.75] },
      color: waystoneActive ? "#91d8bb" : "#68736f",
      transform: { translation: [waystone.x, 1.35, waystone.z] },
    },
    {
      id: "waystone-cap",
      geometry: { kind: "sphere", radius: 0.42 },
      color: waystoneActive ? "#d5fff0" : "#8a9690",
      transform: { translation: [waystone.x, 2.75, waystone.z] },
    },
  ];
}

function resize() {
  const width = Math.max(canvas.clientWidth, 1);
  const height = Math.max(canvas.clientHeight, 1);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  renderer.setSize(width, height, window.devicePixelRatio);
}

function frame(now: number) {
  const deltaSeconds = Math.min((now - lastTime) / 1000, 0.05);
  lastTime = now;
  accumulatedTicks += deltaSeconds * TICK_HZ;
  while (accumulatedTicks >= 1) {
    updateMovement(1 / TICK_HZ);
    accumulatedTicks -= 1;
    demoTick += 1n;
    publishDemoSnapshot();
  }
  const rendered = snapshots.sample(demoTick > 0n ? demoTick - 1n : 0n, accumulatedTicks)[0];
  if (!rendered) throw new Error("Demo snapshot is missing its player");
  const renderX = rendered.position[0] / UNITS_PER_METRE;
  const renderZ = rendered.position[2] / UNITS_PER_METRE;
  target.set(renderX, 0.9, renderZ);
  const desiredCamera = new THREE.Vector3(renderX + 8.5, 7.6, renderZ + 10.5);
  camera.position.lerp(desiredCamera, 1 - Math.pow(0.001, deltaSeconds));
  camera.lookAt(target);
  camera.updateMatrixWorld(true);

  const nearby = nearWaystone();
  prompt.hidden = !nearby;
  prompt.innerHTML = waystoneActive
    ? "<kbd>E</kbd> deactivate waystone"
    : "<kbd>E</kbd> activate waystone";

  renderer.render({
    camera: {
      viewMatrix: [...camera.matrixWorldInverse.elements] as Matrix4Values,
      projectionMatrix: webGpuProjectionFromThree(),
    },
    nodes: [...staticNodes, ...dynamicNodes(rendered.position)],
  });

  requestAnimationFrame(frame);
}

window.addEventListener("keydown", (event) => {
  keys.add(event.code);
  if (event.code === "KeyE" && !event.repeat) interact();
  if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space"].includes(event.code)) {
    event.preventDefault();
  }
});
window.addEventListener("keyup", (event) => keys.delete(event.code));
window.addEventListener("blur", () => keys.clear());
window.addEventListener("resize", resize);

resize();
requestAnimationFrame(frame);
