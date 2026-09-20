import * as THREE from "three";
import { createThreeSceneRenderer } from "https://cdn.jsdelivr.net/gh/moritzbrantner/3d-lab@8187e506f24682dae550284070e45ac33e53ad9c/packages/renderer/index.js";
import initWasm, { DemoSimulation, tick_hz } from "./pkg/mmorpg_web.js";
import { attachDemoInputBindings } from "./input-bindings.js";
import {
  LANDMARKS,
  buildLandmarkNodes,
  buildPlayerNodes,
  buildStaticWorldNodes,
  nearestLandmark,
  toWorldPosition,
} from "./world.js";

const MAX_FRAME_MS = 250;
const MAX_TICKS_PER_FRAME = 5;
const canvas = document.querySelector("#game-canvas");
const loading = document.querySelector("#loading-state");
const statusLabel = document.querySelector("#runtime-status");
const tickLabel = document.querySelector("#tick-value");
const positionLabel = document.querySelector("#position-value");
const objectiveLabel = document.querySelector("#objective-value");
const interaction = document.querySelector("#interaction-prompt");
const interactionTitle = document.querySelector("#interaction-title");
const log = document.querySelector("#event-log");
const resetButton = document.querySelector("#reset-demo");

const completed = new Set();
const staticNodes = buildStaticWorldNodes();
let simulation;
let renderer;
let controls;
let currentSnapshot;
let lastFrameMs;
let tickAccumulator = 0;
let tickMs = 1000 / 30;
let stopped = false;

function appendLog(message) {
  const item = document.createElement("li");
  item.textContent = message;
  log.prepend(item);
  while (log.children.length > 4) log.lastElementChild?.remove();
}

function fail(message, error) {
  console.error(message, error);
  statusLabel.textContent = "Unavailable";
  statusLabel.dataset.state = "error";
  loading.hidden = false;
  loading.querySelector("strong").textContent = message;
  loading.querySelector("span:last-child").textContent = String(error?.message ?? error ?? "Unknown browser error");
}

function snapshot() {
  return JSON.parse(simulation.snapshot_json());
}

function updateHud(state) {
  tickLabel.textContent = state.tick.toLocaleString();
  const [x, , z] = toWorldPosition(state.position);
  positionLabel.textContent = `${x.toFixed(1)}, ${z.toFixed(1)}`;
  objectiveLabel.textContent = `${completed.size} / ${LANDMARKS.length}`;

  const nearby = nearestLandmark(state.position);
  if (!nearby) {
    interaction.hidden = true;
    return;
  }
  interaction.hidden = false;
  const alreadyDone = completed.has(nearby.landmark.id);
  interactionTitle.textContent = alreadyDone
    ? `${nearby.landmark.title} already inspected`
    : nearby.landmark.prompt;
  interaction.dataset.complete = String(alreadyDone);
}

function cameraFrame(state) {
  const [x, , z] = toWorldPosition(state.position);
  const width = Math.max(1, canvas.clientWidth);
  const height = Math.max(1, canvas.clientHeight);
  const camera = cameraFrame.camera ??= new THREE.PerspectiveCamera(48, width / height, 0.1, 120);
  camera.coordinateSystem = THREE.WebGPUCoordinateSystem;
  camera.aspect = width / height;
  camera.position.set(x + 8.7, 8.3, z + 10.5);
  camera.lookAt(x, 0.65, z);
  camera.updateProjectionMatrix();
  camera.updateMatrixWorld(true);
  return {
    viewMatrix: camera.matrixWorldInverse.toArray(),
    projectionMatrix: camera.projectionMatrix.toArray(),
  };
}

function render(state) {
  const width = Math.max(1, canvas.clientWidth);
  const height = Math.max(1, canvas.clientHeight);
  renderer.setSize(width, height, window.devicePixelRatio || 1);
  renderer.render({
    camera: cameraFrame(state),
    nodes: [...staticNodes, ...buildLandmarkNodes(completed), ...buildPlayerNodes(state.position)],
  });
  updateHud(state);
}

function interact() {
  if (!currentSnapshot) return;
  const nearby = nearestLandmark(currentSnapshot.position);
  if (!nearby) {
    appendLog("Nothing nearby to interact with.");
    return;
  }
  if (completed.has(nearby.landmark.id)) {
    appendLog(`${nearby.landmark.title}: already inspected.`);
    return;
  }

  // Tech-demo progress is presentation state only. Durable character/world mutations belong in MMORPG domain boundaries.
  completed.add(nearby.landmark.id);
  appendLog(nearby.landmark.result);
  if (completed.size === LANDMARKS.length) {
    appendLog("Frontier survey complete. The small Pages slice is fully explored.");
  }
  render(currentSnapshot);
}

function frame(timestampMs) {
  if (stopped) return;
  if (lastFrameMs === undefined) lastFrameMs = timestampMs;
  tickAccumulator += Math.min(MAX_FRAME_MS, Math.max(0, timestampMs - lastFrameMs));
  lastFrameMs = timestampMs;

  let advanced = 0;
  while (tickAccumulator >= tickMs && advanced < MAX_TICKS_PER_FRAME) {
    simulation.advance_ticks(1);
    tickAccumulator -= tickMs;
    advanced += 1;
  }
  if (advanced === MAX_TICKS_PER_FRAME && tickAccumulator >= tickMs) tickAccumulator = 0;

  currentSnapshot = snapshot();
  render(currentSnapshot);
  requestAnimationFrame(frame);
}

async function start() {
  try {
    await initWasm();
    simulation = new DemoSimulation();
    tickMs = 1000 / tick_hz();
    renderer = createThreeSceneRenderer(canvas, {
      antialias: true,
      background: 0x101914,
      pixelRatioLimit: 2,
    });
    currentSnapshot = snapshot();
    render(currentSnapshot);

    controls = attachDemoInputBindings({
      target: canvas,
      onMovement: (x, z) => simulation.set_movement(x, z),
      onInteract: interact,
      onUnavailable: (error) => fail("Shared controls could not be loaded.", error),
    });
    await controls.ready;

    canvas.addEventListener("pointerdown", () => canvas.focus());
    resetButton.addEventListener("click", () => {
      simulation.reset();
      completed.clear();
      lastFrameMs = undefined;
      tickAccumulator = 0;
      currentSnapshot = snapshot();
      appendLog("Tech demo reset.");
      render(currentSnapshot);
      canvas.focus();
    });

    statusLabel.textContent = `Local core · ${tick_hz()} Hz`;
    statusLabel.dataset.state = "ready";
    loading.hidden = true;
    canvas.focus();
    requestAnimationFrame(frame);
  } catch (error) {
    fail("The tech demo could not start.", error);
  }
}

window.addEventListener("beforeunload", () => {
  stopped = true;
  controls?.destroy();
  renderer?.dispose();
});

start();
