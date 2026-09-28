import { LocalZoneSource, type LocalZoneHandle } from "./local-zone-source";
import { createSceneryProvider, type SceneryContent, type SceneryProvider } from "./scenery";

/** The initialised wasm-bindgen module surface the local world needs. */
export type LocalZoneModule = SceneryContent & {
  LocalZone: new () => LocalZoneHandle & { contentRevision(): bigint };
};

export type LocalWorld = { source: LocalZoneSource; scenery: SceneryProvider };

/**
 * One local zone per page plus the scenery of the same content revision.
 * Mismatched content stops entry instead of drawing a different world.
 */
export function createLocalWorld(module: LocalZoneModule): LocalWorld {
  const zone = new module.LocalZone();
  const scenery = createSceneryProvider(module);
  if (scenery.scenery.contentRevision !== zone.contentRevision()) {
    throw new Error(
      `Scenery content revision ${scenery.scenery.contentRevision} does not match zone revision ${zone.contentRevision()}.`,
    );
  }
  return { source: new LocalZoneSource(zone), scenery };
}
