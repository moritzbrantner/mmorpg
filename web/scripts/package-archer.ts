import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { assertCleanSourceCheckout } from "./asset-source-checkout";

const packageDirectory = fileURLToPath(new URL("../assets/medieval-character-kit/", import.meta.url));
const source = JSON.parse(await readFile(path.join(packageDirectory, "source.json"), "utf8")) as {
  repository: string; commit: string; generator: string; archetype: string;
};
const checkout = path.resolve(process.argv[2] ?? "../asset-tooling");
const mode = process.argv[3] ?? "--check";
if (mode !== "--check" && mode !== "--write") {
  throw new Error("usage: bun scripts/package-archer.ts [asset-tooling-checkout] [--check|--write]");
}
assertCleanSourceCheckout(checkout, source.commit);
const kit = await import(pathToFileURL(path.join(checkout, "src/medieval-character-kit.ts")).href);
const materialKit = await import(pathToFileURL(path.join(checkout, "src/medieval-character-materials.ts")).href);
const generated = kit.generateMedievalCharacterObj(source.archetype);
const manifest = kit.buildMedievalCharacterKitManifest();
const materials = materialKit.buildMedievalCharacterMaterialManifest();
const entry = manifest.assets.find((asset: { id: string }) => asset.id === `medieval.${source.archetype}`);
const digest = createHash("sha256").update(generated.bytes).digest("hex");
if (!entry || entry.sha256 !== digest || entry.byteLength !== generated.bytes.length) {
  throw new Error("asset-tooling generator and manifest disagree");
}
const packagedManifest = { ...manifest, assets: [entry] };
const packagedMaterials = { ...materials, bindings: { archer: materials.bindings.archer } };
for (const [name, expected] of [
  ["archer.obj", generated.bytes],
  ["manifest.json", Buffer.from(`${JSON.stringify(packagedManifest, null, 2)}\n`)],
  ["materials.json", Buffer.from(`${JSON.stringify(packagedMaterials, null, 2)}\n`)],
] as const) {
  const destination = path.join(packageDirectory, name);
  if (mode === "--write") {
    await writeFile(destination, expected);
  } else if (!(await readFile(destination)).equals(expected)) {
    throw new Error(`${name} differs from asset-tooling ${source.commit}`);
  }
}
console.log(`archer package ${mode === "--check" ? "verified" : "written"}: ${digest}`);
