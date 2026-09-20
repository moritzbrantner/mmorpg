export const WORLD_SCALE = 0.01;
export const INTERACTION_RADIUS = 1.25;

export const LANDMARKS = Object.freeze([
  { id: "boundary-stone", title: "Boundary stone", prompt: "Read the boundary stone", result: "The stone marks the old road into the northern marches.", position: [3.4, 2.8], color: 0xb7a27a },
  { id: "old-well", title: "Old well", prompt: "Draw water from the old well", result: "The bucket comes back cold and clear. The well is still usable.", position: [-4.2, 5.2], color: 0x6e8492 },
  { id: "watchfire", title: "Watchfire", prompt: "Light the watchfire", result: "A small flame catches. For the demo, the frontier post is awake again.", position: [5.7, -4.9], color: 0x8f603f },
]);

const node = (id, geometry, color, translation, scale) => ({
  id,
  geometry,
  color,
  transform: { translation, ...(scale ? { scale } : {}) },
});

function tree(id, x, z, size = 1) {
  return [
    node(`${id}-trunk`, { kind: "cylinder", radius: 0.16, height: 1.55 }, 0x4c3828, [x, 0.78, z], [size, size, size]),
    node(`${id}-crown`, { kind: "sphere", radius: 0.78 }, 0x28503a, [x, 1.85 * size, z], [size, size, size]),
  ];
}

export function buildStaticWorldNodes() {
  const nodes = [
    node("ground", { kind: "box", size: [34, 0.15, 34] }, 0x243a2b, [0, -0.1, 0]),
    node("path-north", { kind: "box", size: [2.2, 0.03, 15] }, 0x665b45, [0.9, 0.0, 1.2]),
    node("path-cross", { kind: "box", size: [13, 0.035, 1.7] }, 0x665b45, [-0.7, 0.005, 3.7]),
    node("ruin-wall-a", { kind: "box", size: [4.2, 1.1, 0.45] }, 0x777467, [-7.2, 0.55, -6.2]),
    node("ruin-wall-b", { kind: "box", size: [0.45, 1.1, 3.3] }, 0x777467, [-9.05, 0.55, -4.8]),
    node("ridge-a", { kind: "box", size: [8, 2.3, 2.8] }, 0x34433a, [9.8, 1.05, 11.4]),
    node("ridge-b", { kind: "box", size: [5.5, 3.0, 2.4] }, 0x303d35, [4.2, 1.4, 14.2]),
  ];
  [
    [-10, -10, 1.2], [-5, -12, 1], [1.5, -12.5, .9], [7.5, -10.5, 1.2],
    [11, -4, 1], [11, 5, 1.1], [7, 11, 1], [0, 12, 1.1],
    [-7, 10.5, 1], [-12, 3, 1.1], [-11, -4, .9], [-5.8, 1.2, .85],
  ].forEach(([x, z, size], index) => nodes.push(...tree(`tree-${index}`, x, z, size)));
  return Object.freeze(nodes);
}

export function buildLandmarkNodes(completed) {
  const nodes = [];
  for (const landmark of LANDMARKS) {
    const [x, z] = landmark.position;
    const color = completed.has(landmark.id) ? 0xe0b85c : landmark.color;
    if (landmark.id === "old-well") {
      nodes.push(
        node(`${landmark.id}-base`, { kind: "cylinder", radius: 0.7, height: 0.5 }, color, [x, 0.25, z]),
        node(`${landmark.id}-water`, { kind: "cylinder", radius: 0.48, height: 0.03 }, 0x365d6e, [x, 0.51, z]),
      );
    } else if (landmark.id === "watchfire") {
      nodes.push(
        node(`${landmark.id}-bowl`, { kind: "cylinder", radius: 0.6, height: 0.25 }, 0x4e4439, [x, 0.2, z]),
        node(`${landmark.id}-signal`, { kind: "sphere", radius: 0.36 }, color, [x, 0.72, z], [1, 1.4, 1]),
      );
    } else {
      nodes.push(
        node(`${landmark.id}-stone`, { kind: "box", size: [0.9, 1.7, 0.55] }, color, [x, 0.85, z]),
        node(`${landmark.id}-cap`, { kind: "box", size: [1.2, 0.24, 0.78] }, 0x8d8979, [x, 1.72, z]),
      );
    }
  }
  return nodes;
}

export function buildPlayerNodes(position) {
  const [x, y, z] = toWorldPosition(position);
  return [
    node("player-body", { kind: "cylinder", radius: 0.28, height: 0.85 }, 0x304e65, [x, Math.max(0.52, y), z]),
    node("player-head", { kind: "sphere", radius: 0.23 }, 0xd1b190, [x, Math.max(1.08, y + 0.55), z]),
    node("player-cloak", { kind: "box", size: [0.62, 0.7, 0.16] }, 0x763e39, [x, Math.max(0.66, y + 0.05), z + 0.22]),
  ];
}

export function toWorldPosition(position) {
  return [position[0] * WORLD_SCALE, position[1] * WORLD_SCALE, position[2] * WORLD_SCALE];
}

export function nearestLandmark(position) {
  const [x, , z] = toWorldPosition(position);
  let nearest = null;
  let distance = Infinity;
  for (const landmark of LANDMARKS) {
    const candidate = Math.hypot(landmark.position[0] - x, landmark.position[1] - z);
    if (candidate < distance) {
      nearest = landmark;
      distance = candidate;
    }
  }
  return distance <= INTERACTION_RADIUS ? { landmark: nearest, distance } : null;
}
