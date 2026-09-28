import { describe, expect, test } from "bun:test";
import {
  BLADE,
  BOX,
  GABLE,
  MeshBuilder,
  PYRAMID,
  applyPoint,
  blob,
  compose,
  disc,
  frustum,
  hash01,
  rotateX,
  rotateY,
  rotateZ,
  scale,
  translate,
  type Shape,
} from "../src/world/mesh-builder";
import { expectValidMesh, triangleNormals } from "./support/geometry";

/** Every face of a closed convex-ish shape points away from `centre`. */
function expectOutward(shape: Shape, centre: readonly [number, number, number], label: string): void {
  triangleNormals(shape).forEach((normal, triangle) => {
    const indices = [0, 1, 2].map((offset) => shape.indices[triangle * 3 + offset]!);
    const face = indices.map((index) => shape.positions[index]!);
    const middle = [0, 1, 2].map((axis) => face.reduce((sum, point) => sum + point[axis]!, 0) / 3);
    const outward = [0, 1, 2].reduce((sum, axis) => sum + (middle[axis]! - centre[axis]!) * normal[axis]!, 0);
    expect(outward, `${label} triangle ${triangle}`).toBeGreaterThan(0);
  });
}

describe("unit shapes", () => {
  test("are valid indexed meshes with unit normals and outward faces", () => {
    const closed: [string, Shape, [number, number, number]][] = [
      ["box", BOX, [0, 0, 0]],
      ["prism", frustum(6), [0, 0.5, 0]],
      ["smooth frustum", frustum(8, 0.6, { smooth: true }), [0, 0.5, 0]],
      ["cone", frustum(7, 0), [0, 0.3, 0]],
      ["icosphere", blob(0), [0, 0, 0]],
      ["subdivided sphere", blob(1), [0, 0, 0]],
      ["flat rock", blob(0, { smooth: false, jitter: 0.25, seed: 3 }), [0, 0, 0]],
    ];
    for (const [label, shape, centre] of closed) {
      expectValidMesh(shape, label);
      expectOutward(shape, centre, label);
    }
    for (const [label, shape] of [["gable", GABLE], ["pyramid", PYRAMID], ["blade", BLADE], ["disc", disc(12)]] as const) {
      expectValidMesh(shape, label);
      expect(triangleNormals(shape).every((normal) => normal[1] >= -1e-9), label).toBe(true);
    }
    expect(blob(1).positions.length).toBe(42);
    expect(frustum(6)).toBe(frustum(6));
  });

  test("jitter is seeded and deterministic", () => {
    const a = blob(0, { jitter: 0.2, seed: 5 });
    expect(blob(0, { jitter: 0.2, seed: 5 }).positions).toEqual(a.positions);
    expect(blob(0, { jitter: 0.2, seed: 6 }).positions).not.toEqual(a.positions);
    for (let seed = 0; seed < 50; seed += 1) {
      const value = hash01(seed, seed * 3, -seed);
      expect(value).toBeGreaterThanOrEqual(0);
      expect(value).toBeLessThan(1);
      expect(hash01(seed, seed * 3, -seed)).toBe(value);
    }
  });
});

describe("mesh builder", () => {
  test("composes transforms: scale first, then rotate, then translate", () => {
    const transform = compose(translate(10, 0, 0), rotateY(Math.PI / 2), scale(2, 1, 1));
    const [x, y, z] = applyPoint(transform, [1, 0, 0]);
    // (1, 0, 0) scales to (2, 0, 0); a quarter yaw turns +X toward -Z.
    expect(x).toBeCloseTo(10, 9);
    expect(y).toBeCloseTo(0, 9);
    expect(z).toBeCloseTo(-2, 9);
  });

  test("keeps normals perpendicular under non-uniform scale and faces outward when mirrored", () => {
    const builder = new MeshBuilder();
    builder.add(BOX, compose(rotateZ(0.3), rotateX(0.2), scale(3, 0.5, 1)));
    builder.add(BOX, compose(translate(5, 0, 0), scale(-1, 1, 1)));
    const mesh = builder.data();
    expectValidMesh(mesh);
    const faces = triangleNormals(mesh);
    faces.forEach((geometric, triangle) => {
      const shading = mesh.normals[mesh.indices[triangle * 3]!]!;
      const length = Math.hypot(...geometric);
      const cosine = geometric.reduce((sum, value, axis) => sum + value * shading[axis]!, 0) / length;
      expect(cosine, `triangle ${triangle}`).toBeCloseTo(1, 6);
    });
    expect(builder.vertexCount).toBe(48);
  });
});
