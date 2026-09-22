import * as THREE from "three";
import {
  createThreeSceneRenderer,
  type Matrix4Values,
  type RendererSceneNode,
} from "@moritzbrantner/three-d-renderer";
import {
  DEFAULT_CHARACTER_APPEARANCE,
  equipmentForAppearance,
  hatOption,
  isHatStyle,
  loadCharacter,
  rotateYawOffset,
  saveCharacter,
  type CharacterAppearance,
  type HatStyle,
} from "./character-customization";
import {
  PREVIEW_CHARACTER,
  enterPreviewWorld,
  initialEntryState,
  type CharacterPreview,
  type EntryState,
} from "./character-selection";
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
const characterSelect = requireElement<HTMLElement>("#character-select");
const enterWorldButton = requireElement<HTMLButtonElement>("#enter-world");
const saveCharacterButton = requireElement<HTMLButtonElement>("#save-character");
const loadCharacterButton = requireElement<HTMLButtonElement>("#load-character");
const saveStatus = requireElement<HTMLElement>("#save-status");
const equipmentList = requireElement<HTMLElement>("#equipment-list");
const characterName = requireElement<HTMLElement>("#character-name");
const characterSubtitle = requireElement<HTMLElement>("#character-subtitle");
const characterLocation = requireElement<HTMLElement>("#character-location");
const fallbackCharacter = requireElement<HTMLElement>("#character-stage-fallback");
const hatButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-hat-style]")];
const worldUi = [...document.querySelectorAll<HTMLElement>("[data-world-ui]")];

const renderer = createThreeSceneRenderer(canvas, {
  background: "#111820",
  antialias: true,
  pixelRatioLimit: 2,
});

fallbackCharacter.hidden = true;

type Vec2 = { x: number; z: number };
type RotationQuaternion = [number, number, number, number];
type LocalPoint = (x: number, y: number, z: number) => [number, number, number];

const player: Vec2 = { x: -5.5, z: 4.5 };
let facing = Math.PI * 0.15;
let waystoneActive = false;
let lastTime = performance.now();
const keys = new Set<string>();
let entryState: EntryState = initialEntryState();
let characterAppearance: CharacterAppearance = { ...DEFAULT_CHARACTER_APPEARANCE };

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

const selectionStageNodes: RendererSceneNode[] = [
  node("selection-floor", "cylinder", [-1.2, -0.12, 0], "#27332f", [3.0, 0.28, 0]),
  node("selection-floor-inner", "cylinder", [-1.2, 0.04, 0], "#495b50", [2.25, 0.12, 0]),
  node("selection-column-left", "box", [-5.4, 2.2, -2.0], "#313b39", [1.1, 4.4, 1.1]),
  node("selection-column-right", "box", [3.0, 2.2, -2.0], "#313b39", [1.1, 4.4, 1.1]),
  node("selection-plinth-left", "box", [-5.4, 0.35, -2.0], "#516057", [1.7, 0.7, 1.7]),
  node("selection-plinth-right", "box", [3.0, 0.35, -2.0], "#516057", [1.7, 0.7, 1.7]),
  node("selection-lantern-left", "sphere", [-4.8, 3.8, -1.2], "#c4a35a", [0.16, 0, 0]),
  node("selection-lantern-right", "sphere", [2.4, 3.8, -1.2], "#c4a35a", [0.16, 0, 0]),
  node("selection-backdrop", "box", [-1.2, 2.25, -4.0], "#1d292a", [9.6, 4.8, 0.3]),
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
  const rotationQuaternion: RotationQuaternion = [0, Math.sin(halfYaw), 0, Math.cos(halfYaw)];
  const [swordOffsetX, swordOffsetZ] = rotateYawOffset(facing, 0.58, 0.04);
  return [
    {
      id: "player",
      geometry: { kind: "cylinder", radius: 0.42, height: 1.65 },
      color: "#718d84",
      transform: { translation: [x, y, z], rotationQuaternion },
    },
    {
      id: "player-head",
      geometry: { kind: "sphere", radius: 0.34 },
      color: "#c49b78",
      transform: { translation: [x, y + 1.07, z] },
    },
    {
      id: "player-shoulders",
      geometry: { kind: "box", size: [1.08, 0.24, 0.42] },
      color: "#6e8f84",
      transform: { translation: [x, y + 0.56, z], rotationQuaternion },
    },
    {
      id: "player-sword",
      geometry: { kind: "box", size: [0.12, 1.35, 0.08] },
      color: "#c7b66d",
      transform: {
        translation: [x + swordOffsetX, y + 0.15, z + swordOffsetZ],
        rotationQuaternion,
      },
    },
    ...worldHatNodes(x, y, z, rotationQuaternion, characterAppearance.hat),
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

function worldHatNodes(
  x: number,
  y: number,
  z: number,
  rotationQuaternion: RotationQuaternion,
  hat: HatStyle,
): RendererSceneNode[] {
  if (hat === "wayfarer-hood") {
    return [{
      id: "player-hat-hood",
      geometry: { kind: "cylinder", radius: 0.36, height: 0.2 },
      color: "#b7c6bd",
      transform: { translation: [x, y + 1.34, z], rotationQuaternion },
    }];
  }
  if (hat === "ranger-cap") {
    return [
      {
        id: "player-hat-cap-brim",
        geometry: { kind: "cylinder", radius: 0.45, height: 0.08 },
        color: "#6f875f",
        transform: { translation: [x, y + 1.32, z], rotationQuaternion },
      },
      {
        id: "player-hat-cap-crown",
        geometry: { kind: "cylinder", radius: 0.29, height: 0.2 },
        color: "#5d7351",
        transform: { translation: [x, y + 1.42, z], rotationQuaternion },
      },
    ];
  }
  return [
    {
      id: "player-hat-helm",
      geometry: { kind: "cylinder", radius: 0.35, height: 0.28 },
      color: "#858e91",
      transform: { translation: [x, y + 1.33, z], rotationQuaternion },
    },
    {
      id: "player-hat-crest",
      geometry: { kind: "box", size: [0.1, 0.36, 0.34] },
      color: "#aab0b2",
      transform: { translation: [x, y + 1.58, z], rotationQuaternion },
    },
  ];
}

function selectionCharacterNodes(now: number): RendererSceneNode[] {
  const baseX = -1.2;
  const idleYaw = Math.sin(now / 3400) * 0.16 - 0.18;
  const halfYaw = idleYaw / 2;
  const rotationQuaternion: RotationQuaternion = [0, Math.sin(halfYaw), 0, Math.cos(halfYaw)];
  const localPoint: LocalPoint = (x, y, z) => {
    const cos = Math.cos(idleYaw);
    const sin = Math.sin(idleYaw);
    return [baseX + x * cos + z * sin, y, -x * sin + z * cos];
  };

  return [
    {
      id: "preview-body",
      geometry: { kind: "cylinder", radius: 0.48, height: 1.7 },
      color: "#718d84",
      transform: { translation: localPoint(0, 1.28, 0), rotationQuaternion },
    },
    {
      id: "preview-chest",
      geometry: { kind: "box", size: [0.86, 0.82, 0.5] },
      color: "#58746b",
      transform: { translation: localPoint(0, 1.58, 0), rotationQuaternion },
    },
    {
      id: "preview-head",
      geometry: { kind: "sphere", radius: 0.34 },
      color: "#c49b78",
      transform: { translation: localPoint(0, 2.48, 0), rotationQuaternion },
    },
    ...previewHatNodes(characterAppearance.hat, localPoint, rotationQuaternion),
    {
      id: "preview-face",
      geometry: { kind: "sphere", radius: 0.27 },
      color: "#c49b78",
      transform: { translation: localPoint(0, 2.46, -0.18), rotationQuaternion },
    },
    {
      id: "preview-shoulder-left",
      geometry: { kind: "box", size: [0.42, 0.28, 0.62] },
      color: "#6e8f84",
      transform: { translation: localPoint(-0.52, 1.96, 0), rotationQuaternion },
    },
    {
      id: "preview-shoulder-right",
      geometry: { kind: "box", size: [0.42, 0.28, 0.62] },
      color: "#6e8f84",
      transform: { translation: localPoint(0.52, 1.96, 0), rotationQuaternion },
    },
    {
      id: "preview-arm-left",
      geometry: { kind: "cylinder", radius: 0.16, height: 1.02 },
      color: "#718d84",
      transform: { translation: localPoint(-0.52, 1.37, 0), rotationQuaternion },
    },
    {
      id: "preview-arm-right",
      geometry: { kind: "cylinder", radius: 0.16, height: 1.02 },
      color: "#718d84",
      transform: { translation: localPoint(0.52, 1.37, 0), rotationQuaternion },
    },
    {
      id: "preview-boot-left",
      geometry: { kind: "box", size: [0.34, 0.62, 0.48] },
      color: "#745d49",
      transform: { translation: localPoint(-0.24, 0.38, -0.06), rotationQuaternion },
    },
    {
      id: "preview-boot-right",
      geometry: { kind: "box", size: [0.34, 0.62, 0.48] },
      color: "#745d49",
      transform: { translation: localPoint(0.24, 0.38, -0.06), rotationQuaternion },
    },
    {
      id: "preview-cloak",
      geometry: { kind: "box", size: [0.82, 1.45, 0.08] },
      color: "#425c56",
      transform: { translation: localPoint(0, 1.37, 0.34), rotationQuaternion },
    },
    {
      id: "preview-sword-blade",
      geometry: { kind: "box", size: [0.13, 1.78, 0.09] },
      color: "#d3d6cf",
      transform: { translation: localPoint(0.82, 1.1, -0.02), rotationQuaternion },
    },
    {
      id: "preview-sword-hilt",
      geometry: { kind: "box", size: [0.58, 0.1, 0.12] },
      color: "#c7b66d",
      transform: { translation: localPoint(0.82, 1.93, -0.02), rotationQuaternion },
    },
    {
      id: "preview-sword-grip",
      geometry: { kind: "cylinder", radius: 0.08, height: 0.44 },
      color: "#715744",
      transform: { translation: localPoint(0.82, 2.14, -0.02), rotationQuaternion },
    },
  ];
}

function previewHatNodes(
  hat: HatStyle,
  localPoint: LocalPoint,
  rotationQuaternion: RotationQuaternion,
): RendererSceneNode[] {
  if (hat === "wayfarer-hood") {
    return [{
      id: "preview-hat-hood",
      geometry: { kind: "sphere", radius: 0.4 },
      color: "#b7c6bd",
      transform: { translation: localPoint(0, 2.56, 0.06), rotationQuaternion },
    }];
  }
  if (hat === "ranger-cap") {
    return [
      {
        id: "preview-hat-cap-brim",
        geometry: { kind: "cylinder", radius: 0.5, height: 0.08 },
        color: "#6f875f",
        transform: { translation: localPoint(0, 2.72, 0), rotationQuaternion },
      },
      {
        id: "preview-hat-cap-crown",
        geometry: { kind: "cylinder", radius: 0.31, height: 0.25 },
        color: "#5d7351",
        transform: { translation: localPoint(0, 2.84, 0.04), rotationQuaternion },
      },
    ];
  }
  return [
    {
      id: "preview-hat-helm",
      geometry: { kind: "sphere", radius: 0.39 },
      color: "#858e91",
      transform: { translation: localPoint(0, 2.56, 0.04), rotationQuaternion },
    },
    {
      id: "preview-hat-brow",
      geometry: { kind: "box", size: [0.62, 0.1, 0.12] },
      color: "#aab0b2",
      transform: { translation: localPoint(0, 2.58, -0.33), rotationQuaternion },
    },
    {
      id: "preview-hat-crest",
      geometry: { kind: "box", size: [0.12, 0.44, 0.42] },
      color: "#aab0b2",
      transform: { translation: localPoint(0, 2.94, 0.04), rotationQuaternion },
    },
  ];
}

function renderCharacterDetails(character: CharacterPreview, appearance: CharacterAppearance) {
  characterName.textContent = character.name;
  characterSubtitle.textContent = `Level ${character.level} · ${character.race} ${character.className}`;
  characterLocation.textContent = character.location;
  const equipment = equipmentForAppearance(character, appearance);
  equipmentList.replaceChildren(...equipment.map((item) => {
    const article = document.createElement("article");
    article.className = "equipment-item";
    const swatch = document.createElement("span");
    swatch.className = "equipment-swatch";
    swatch.style.setProperty("--equipment-accent", item.accent);
    const copy = document.createElement("span");
    const slot = document.createElement("small");
    slot.textContent = item.slot;
    const name = document.createElement("strong");
    name.textContent = item.name;
    copy.append(slot, name);
    article.append(swatch, copy);
    return article;
  }));
}

function applyAppearance(nextAppearance: CharacterAppearance, message: string) {
  characterAppearance = { ...nextAppearance };
  characterSelect.dataset.hat = characterAppearance.hat;
  renderCharacterDetails(PREVIEW_CHARACTER, characterAppearance);
  for (const button of hatButtons) {
    const selected = button.dataset.hatStyle === characterAppearance.hat;
    button.classList.toggle("is-selected", selected);
    button.setAttribute("aria-pressed", String(selected));
  }
  saveStatus.textContent = message;
}

function saveCurrentCharacter() {
  try {
    const saved = saveCharacter(localStorage, PREVIEW_CHARACTER.id, characterAppearance);
    saveStatus.textContent = `Saved locally · ${hatOption(saved.appearance.hat).name}`;
  } catch {
    saveStatus.textContent = "Browser storage is unavailable; character was not saved.";
  }
}

function loadSavedCharacter() {
  try {
    const saved = loadCharacter(localStorage, PREVIEW_CHARACTER.id);
    if (!saved) {
      saveStatus.textContent = "No local save exists for this character yet.";
      return;
    }
    applyAppearance(saved.appearance, `Loaded local save · ${hatOption(saved.appearance.hat).name}`);
  } catch {
    saveStatus.textContent = "Local save is invalid and was not loaded.";
  }
}

function resize() {
  const width = Math.max(canvas.clientWidth, 1);
  const height = Math.max(canvas.clientHeight, 1);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  renderer.setSize(width, height, window.devicePixelRatio);
}

function renderSelection(now: number) {
  target.set(-1.2, 1.45, 0);
  camera.position.set(3.3, 2.7, 7.1);
  camera.lookAt(target);
  camera.updateMatrixWorld(true);
  renderer.render({
    camera: {
      viewMatrix: [...camera.matrixWorldInverse.elements] as Matrix4Values,
      projectionMatrix: webGpuProjectionFromThree(),
    },
    nodes: [...selectionStageNodes, ...selectionCharacterNodes(now)],
  });
}

function renderWorld(deltaSeconds: number) {
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
}

function frame(now: number) {
  const deltaSeconds = Math.min((now - lastTime) / 1000, 0.05);
  lastTime = now;
  if (entryState.phase === "character-selection") {
    renderSelection(now);
  } else {
    renderWorld(deltaSeconds);
  }
  requestAnimationFrame(frame);
}

function enterWorld() {
  entryState = enterPreviewWorld(entryState);
  characterSelect.hidden = true;
  for (const element of worldUi) element.hidden = false;
  status.textContent = "Exploring";
  objective.textContent = "Reach the old waystone and activate it.";
  keys.clear();
  lastTime = performance.now();
  canvas.focus();
}

applyAppearance(DEFAULT_CHARACTER_APPEARANCE, "Not saved yet.");
for (const button of hatButtons) {
  button.addEventListener("click", () => {
    const style = button.dataset.hatStyle;
    if (!isHatStyle(style)) return;
    applyAppearance({ hat: style }, `Unsaved change · ${hatOption(style).name}`);
  });
}
saveCharacterButton.addEventListener("click", saveCurrentCharacter);
loadCharacterButton.addEventListener("click", loadSavedCharacter);
enterWorldButton.addEventListener("click", enterWorld);

window.addEventListener("keydown", (event) => {
  if (entryState.phase === "character-selection") {
    if (event.code === "Enter" && !event.repeat) {
      event.preventDefault();
      enterWorld();
    }
    return;
  }
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
