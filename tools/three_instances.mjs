/**
 * Loads .glb files with three.js's GLTFLoader under Node and prints, per file, the
 * instance counts of its instancing nodes (EXT_mesh_gpu_instancing) per group node, the
 * total, and how many instances carry an instance colour (`_COLOR_0`).
 *
 * Usage: node tools/three_instances.mjs <three package dir> <file.glb> [expected total]
 *
 * The three.js package is given by its directory (it is not a dependency of the tools).
 * Images are not decoded under Node; the loader reports them and goes on. With an
 * expected total, the exit code is 1 when the instance count differs.
 */
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [threeDir, file, expected] = process.argv.slice(2);
if (!threeDir || !file) {
  console.error("usage: node tools/three_instances.mjs <three dir> <file.glb> [expected total]");
  process.exit(2);
}
globalThis.self = globalThis;
const silence = console.error;
console.error = () => {};
const { GLTFLoader } = await import(pathToFileURL(join(threeDir, "examples/jsm/loaders/GLTFLoader.js")).href);
const bytes = await readFile(file);
const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
const gltf = await new GLTFLoader().parseAsync(buffer, "");
console.error = silence;
const associations = gltf.parser.associations;
const nodes = new Map();
gltf.scene.traverse((object) => {
  if (!object.isInstancedMesh) {
    return;
  }
  // A mesh of several primitives is a group of instanced meshes, one per primitive.
  let node = object;
  while (node && associations.get(node)?.nodes === undefined) {
    node = node.parent;
  }
  nodes.set(node, { count: object.count, colored: Boolean(object.instanceColor) });
});
let total = 0;
let colored = 0;
const perGroup = new Map();
for (const [node, { count, colored: hasColor }] of nodes) {
  total += count;
  colored += hasColor ? count : 0;
  const group = node?.parent?.name ?? "";
  perGroup.set(group, (perGroup.get(group) ?? 0) + count);
}
for (const [group, count] of perGroup) {
  console.log(`${group}: ${count} instances`);
}
console.log(`${file}: ${total} instances, ${colored} with an instance colour`);
if (expected !== undefined && total !== Number(expected)) {
  console.log(`expected ${expected}`);
  process.exit(1);
}
