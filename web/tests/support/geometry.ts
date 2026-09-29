import { expect } from "bun:test";

type Point = readonly [number, number, number];
export type IndexedMesh = { positions: readonly Point[]; normals?: readonly Point[]; indices: readonly number[] };

/** Asserts what the renderer requires of an indexed mesh, plus unit normals and finite values. */
export function expectValidMesh(mesh: IndexedMesh, label = "mesh"): void {
  expect(mesh.positions.length, label).toBeGreaterThan(0);
  expect(mesh.indices.length % 3, label).toBe(0);
  expect(mesh.indices.length, label).toBeGreaterThan(0);
  for (const index of mesh.indices) {
    if (!Number.isSafeInteger(index) || index < 0 || index >= mesh.positions.length) {
      throw new Error(`${label}: index ${index} is out of bounds for ${mesh.positions.length} positions`);
    }
  }
  for (const position of mesh.positions) {
    if (position.length !== 3 || !position.every(Number.isFinite)) {
      throw new Error(`${label}: position ${position.join(",")} is not finite`);
    }
  }
  if (mesh.normals) {
    expect(mesh.normals.length, label).toBe(mesh.positions.length);
    for (const normal of mesh.normals) {
      const length = Math.hypot(...normal);
      if (!normal.every(Number.isFinite) || Math.abs(length - 1) > 1e-6) {
        throw new Error(`${label}: normal ${normal.join(",")} is not a unit vector`);
      }
    }
  }
}

/** Clips a convex polygon to the side of the plane y = `limit` that `inside` keeps. */
function clipAtHeight(polygon: readonly Point[], limit: number, inside: (y: number) => boolean): Point[] {
  const clipped: Point[] = [];
  polygon.forEach((current, index) => {
    const previous = polygon[(index + polygon.length - 1) % polygon.length]!;
    if (inside(current[1]) !== inside(previous[1])) {
      const t = (limit - previous[1]) / (current[1] - previous[1]);
      clipped.push([previous[0] + (current[0] - previous[0]) * t, limit, previous[2] + (current[2] - previous[2]) * t]);
    }
    if (inside(current[1])) {
      clipped.push(current);
    }
  });
  return clipped;
}

/**
 * XZ corners of every triangle clipped to the band `low` ≤ y ≤ `high`. A
 * wall face that spans the band has no vertex inside it, but its clipped
 * corners still mark where it stands.
 */
export function bandFootprint(mesh: IndexedMesh, low: number, high: number): [number, number][] {
  const points: [number, number][] = [];
  for (let index = 0; index < mesh.indices.length; index += 3) {
    const triangle = [0, 1, 2].map((offset) => mesh.positions[mesh.indices[index + offset]!]!);
    const inBand = clipAtHeight(clipAtHeight(triangle, low, (y) => y >= low), high, (y) => y <= high);
    for (const [x, , z] of inBand) {
      points.push([x, z]);
    }
  }
  return points;
}

/** Geometric (right-handed) normal of each triangle, unnormalised. */
export function triangleNormals(mesh: IndexedMesh): Point[] {
  const normals: Point[] = [];
  for (let index = 0; index < mesh.indices.length; index += 3) {
    const [a, b, c] = [0, 1, 2].map((offset) => mesh.positions[mesh.indices[index + offset]!]!) as [Point, Point, Point];
    const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    const v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    normals.push([u[1]! * v[2]! - u[2]! * v[1]!, u[2]! * v[0]! - u[0]! * v[2]!, u[0]! * v[1]! - u[1]! * v[0]!]);
  }
  return normals;
}
