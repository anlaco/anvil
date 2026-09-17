// The repo's examples as the desktop shell hands them to the engine: the file
// and what it references beside it on disk (src/neighbours.mjs). Since the demo
// bench (ADR-0041) `basica.yaml` names an executor binary, and the loader
// refuses a sequence whose binary it cannot see.

import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { gatherFiles } from "../src/neighbours.mjs";

const EJEMPLOS = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "ejemplos");

const diskReader = {
  exists: async (path) => existsSync(join(EJEMPLOS, path)),
  readText: async (path) =>
    existsSync(join(EJEMPLOS, path)) ? readFile(join(EJEMPLOS, path), "utf8") : null,
};

/** `{ files }` for one example, keyed by the name the engine is given. */
export async function exampleFiles(name) {
  const text = await readFile(join(EJEMPLOS, name), "utf8");
  return gatherFiles(name, text, diskReader);
}
