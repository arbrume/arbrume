/**
 * Runs the Khronos glTF validator on .glb files and fails on any error or warning.
 *
 * Usage: node tools/validate_glb.mjs <file.glb | directory> ...
 * Directories are scanned (not recursively) for *.glb files. Infos and hints are printed but
 * do not fail the run. Images referenced by URI (`arbrume gen --textures files`) are read
 * relative to the file that references them. Install the validator first with
 * `npm ci --prefix tools`.
 */
import { readFile, readdir, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import validator from "gltf-validator";

async function collect(paths) {
  const files = [];
  for (const path of paths) {
    if ((await stat(path)).isDirectory()) {
      const names = (await readdir(path)).filter((name) => name.endsWith(".glb")).sort();
      files.push(...names.map((name) => join(path, name)));
    } else {
      files.push(path);
    }
  }
  return files;
}

async function validate(file) {
  const report = await validator.validateBytes(new Uint8Array(await readFile(file)), {
    uri: file,
    maxIssues: 0,
    externalResourceFunction: async (uri) =>
      new Uint8Array(await readFile(join(dirname(file), decodeURIComponent(uri)))),
  });
  const { numErrors, numWarnings, numInfos, numHints, messages } = report.issues;
  console.log(`${file}: ${numErrors} errors, ${numWarnings} warnings, ${numInfos} infos, ${numHints} hints`);
  for (const m of messages) {
    console.log(`  [${["error", "warning", "info", "hint"][m.severity]}] ${m.code} ${m.pointer ?? ""} ${m.message}`);
  }
  return numErrors + numWarnings;
}

const files = await collect(process.argv.slice(2));
if (files.length === 0) {
  console.error("usage: node tools/validate_glb.mjs <file.glb | directory> ...");
  process.exit(2);
}
let failures = 0;
for (const file of files) {
  failures += await validate(file);
}
console.log(`validator ${validator.version()}: ${files.length} files, ${failures} errors and warnings`);
process.exit(failures === 0 ? 0 : 1);
