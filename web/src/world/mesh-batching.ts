import type { IndexedMeshGeometry, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { MeshBuilder } from "./mesh-builder";
import type { Color } from "./scenery";

/**
 * Static batching: every static part is appended to one mesh per
 * (spatial chunk, colour, cull class), so the scene holds a few hundred
 * nodes however many props it has. Each batch uploads once under a stable
 * resource key; the renderer frustum-culls it by its bounding sphere and
 * `cullBatches` hides far detail through `visible`.
 *
 * This is the seam for instancing (3d-lab #82): a batch key maps one-to-one
 * to what would become an instanced draw of a shared model.
 */
export type CullClass = "always" | "far" | "mid" | "near";

/** Beyond this camera distance (metres, to the batch's bounding circle) a class is hidden. */
export const CULL_DISTANCE_METRES: Readonly<Record<CullClass, number>> = {
  always: Number.POSITIVE_INFINITY,
  far: 420,
  mid: 150,
  near: 75,
};

/**
 * Side of a spatial chunk in metres per cull class: small detail is hidden
 * chunk by chunk close to the camera, so its chunks are small; classes that
 * stay visible farther away use larger chunks and fewer nodes.
 */
export const CHUNK_METRES: Readonly<Record<CullClass, number>> = {
  always: 120,
  far: 80,
  mid: 60,
  near: 40,
};

export type StaticBatch = {
  node: RendererSceneNode & { geometry: IndexedMeshGeometry };
  cull: CullClass;
  /** XZ centre and radius of the batch's vertices, in metres. */
  center: readonly [number, number];
  radius: number;
};

type Bucket = { chunk: string; color: Color; cull: CullClass; builder: MeshBuilder };

export function chunkOf(x: number, z: number, cull: CullClass): string {
  const size = CHUNK_METRES[cull];
  return `${size}m${Math.floor(x / size)}_${Math.floor(z / size)}`;
}

export class SceneBatcher {
  readonly #prefix: string;
  readonly #buckets = new Map<string, Bucket>();

  /** `prefix` makes resource keys unique per scenery source and content revision. */
  constructor(prefix: string) {
    this.#prefix = prefix;
  }

  /**
   * The builder for parts of `color` that belong to the chunk containing
   * `(x, z)` in metres. Callers pass a prop's anchor so a prop stays whole.
   */
  builder(color: Color, x: number, z: number, cull: CullClass): MeshBuilder {
    return this.#bucket(chunkOf(x, z, cull), color, cull);
  }

  /** A builder outside the chunk grid, for scene-wide meshes such as terrain. */
  global(name: string, color: Color, cull: CullClass = "always"): MeshBuilder {
    return this.#bucket(`all-${name}`, color, cull);
  }

  #bucket(chunk: string, color: Color, cull: CullClass): MeshBuilder {
    const key = `${chunk}|${color}|${cull}`;
    let bucket = this.#buckets.get(key);
    if (!bucket) {
      bucket = { chunk, color, cull, builder: new MeshBuilder() };
      this.#buckets.set(key, bucket);
    }
    return bucket.builder;
  }

  /** Non-empty batches in a stable order with stable IDs and resource keys. */
  batches(): StaticBatch[] {
    return [...this.#buckets.values()]
      .filter((bucket) => bucket.builder.indices.length > 0)
      .sort((left, right) =>
        left.chunk.localeCompare(right.chunk) || left.cull.localeCompare(right.cull) || left.color.localeCompare(right.color))
      .map((bucket) => {
        const name = `${bucket.chunk}:${bucket.cull}:${bucket.color.slice(1)}`;
        const { positions, normals, indices } = bucket.builder;
        let minX = Number.POSITIVE_INFINITY;
        let maxX = Number.NEGATIVE_INFINITY;
        let minZ = Number.POSITIVE_INFINITY;
        let maxZ = Number.NEGATIVE_INFINITY;
        for (const [x, , z] of positions) {
          minX = Math.min(minX, x);
          maxX = Math.max(maxX, x);
          minZ = Math.min(minZ, z);
          maxZ = Math.max(maxZ, z);
        }
        const center: [number, number] = [(minX + maxX) / 2, (minZ + maxZ) / 2];
        return {
          node: {
            id: `static-${name}`,
            geometry: { kind: "mesh", resourceKey: `${this.#prefix}:${name}`, positions, normals, indices },
            color: bucket.color,
            transform: { translation: [0, 0, 0] },
          },
          cull: bucket.cull,
          center,
          radius: Math.hypot(maxX - minX, maxZ - minZ) / 2,
        };
      });
  }
}

/** Whether a batch is within its class's distance of the camera eye (XZ, metres). */
export function batchVisible(batch: StaticBatch, eyeX: number, eyeZ: number): boolean {
  const distance = Math.hypot(batch.center[0] - eyeX, batch.center[1] - eyeZ) - batch.radius;
  return distance <= CULL_DISTANCE_METRES[batch.cull];
}
