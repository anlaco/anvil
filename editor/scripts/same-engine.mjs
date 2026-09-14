// Checks, on the packaged artefacts, that the editor and the command line
// carry the same engine (ADR-0031): every WebAssembly module inside the
// editor's `app.asar` must appear byte for byte inside the `anvil` binary.
//
//   node scripts/same-engine.mjs <app.asar> <anvil binary>
//
// Transpiling does not recompile the guest — it takes the component's core
// modules out of their wrapper — so byte identity is the actual promise, not an
// approximation of it. `freshness.mjs` warns when a checkout is about to
// transpile a stale guest; this is what refuses to ship one, because it looks
// at what goes out rather than at timestamps in a working tree.
//
// Exits 0 when every module is found, 1 otherwise, saying which.
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";

const [asarPath, binaryPath] = process.argv.slice(2);
if (!asarPath || !binaryPath) {
  console.error("usage: node scripts/same-engine.mjs <app.asar> <anvil binary>");
  process.exit(2);
}

// Comes with electron-builder, which is what writes the archive in the first
// place.
const asar = createRequire(import.meta.url)("@electron/asar");

const binary = await readFile(binaryPath);
const modules = asar.listPackage(asarPath).filter((f) => f.endsWith(".wasm"));

// No modules at all is a failure, not a pass: an editor that lost its engine
// would otherwise sail through a check that found nothing to compare.
if (modules.length === 0) {
  console.error(`no .wasm modules inside ${asarPath}`);
  process.exit(1);
}

let mismatched = 0;
for (const name of modules) {
  const bytes = asar.extractFile(asarPath, name.replace(/^[/\\]/, ""));
  const at = binary.indexOf(bytes);
  if (at === -1) {
    mismatched++;
    console.error(`DIFFERENT ${name} (${bytes.length} bytes) is not inside ${binaryPath}`);
  } else {
    console.log(`same      ${name} (${bytes.length} bytes) at offset ${at}`);
  }
}

if (mismatched > 0) {
  console.error(`${mismatched} of ${modules.length} module(s) differ: the editor and anvil carry different engines`);
  process.exit(1);
}
console.log(`the editor and ${binaryPath} carry the same engine`);
