// The files a sequence needs beside it, gathered for the engine's in-memory
// filesystem.
//
// The engine loads a sequence the way the native binary does, and the loader
// reads more than the one file: a `sequence_call` by path reads that file.
// Handed only the open document, the editor rejected every call to an external
// subsequence — sequences the binary runs.
//
// It used to mount executor binaries too, because `type: wasm` made the loader
// check one existed. ADR-0046 removed that type: an executor is an address and
// the loader touches no binary, so there is nothing left to mount for one.
//
// So the editor mounts what the sequence references, read through `reader`. In
// the desktop shell that is the disk; in a plain browser there is no disk, no
// reader is given, and the loader's own "does not exist" stands — it is true
// of what the page can see, and the editor does not pretend otherwise.

import { parse } from "yaml";

const PHASES = ["setup", "main", "cleanup"];

/** Nesting a real sequence never reaches; stops a reference cycle. */
const MAX_FILES = 64;

/**
 * The loader's rule for telling a path from an inline subsequence's name
 * (`es_path`, crates/cargador/src/lib.rs). Kept identical: a name read as a
 * path here would mount nothing and change nothing, but a path read as a name
 * would leave a file unmounted that the loader then cannot find.
 */
export function isPath(target) {
  const t = String(target).trim();
  return t.includes("/") || t.includes("\\") || /\.(yseq|yaml|yml)$/.test(t);
}

/**
 * `rel` resolved against the directory `dir`, both relative to the mounted
 * root, with `.` and `..` folded. Null when it climbs above the root: the
 * in-memory filesystem has nothing up there to mount it into.
 */
export function joinRelative(dir, rel) {
  if (/^([a-zA-Z]:)?[\\/]/.test(rel)) return null;
  const out = dir ? dir.split("/") : [];
  for (const part of rel.split(/[\\/]/)) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (out.length === 0) return null;
      out.pop();
    } else {
      out.push(part);
    }
  }
  return out.join("/");
}

const dirOf = (path) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");

/** What one sequence text references: wasm executor binaries and called files. */
export function referencesOf(text) {
  let doc;
  try {
    doc = parse(text);
  } catch {
    return { sequences: [] };
  }
  if (!doc || typeof doc !== "object") return { sequences: [] };

  const sequences = [];
  const walk = (section) => {
    for (const phase of PHASES) {
      for (const step of Array.isArray(section?.[phase]) ? section[phase] : []) {
        if (step?.type === "sequence_call" && typeof step.sequence === "string" && isPath(step.sequence)) {
          sequences.push(step.sequence);
        }
      }
    }
  };
  walk(doc);
  for (const sub of Object.values(doc.subsequences ?? {})) walk(sub);

  return { sequences };
}

/**
 * The `files` map for `runEngine`: the open document under `name`, plus every
 * file it reaches that `reader` finds.
 *
 * `reader.readText(path)` resolves to the text or null; `reader.exists(path)`
 * to a boolean. Paths are relative to the open document's directory. An
 */
export async function gatherFiles(name, text, reader) {
  const files = { [name]: text };
  if (!reader) return files;

  const queue = [[name, text]];
  while (queue.length > 0 && Object.keys(files).length < MAX_FILES) {
    const [path, source] = queue.shift();
    const { sequences } = referencesOf(source);
    const dir = dirOf(path);

    for (const rel of sequences) {
      const target = joinRelative(dir, rel);
      if (target === null || target in files) continue;
      const called = await reader.readText(target);
      if (called === null) continue;
      files[target] = called;
      queue.push([target, called]);
    }
  }
  return files;
}

/**
 * Why a never-saved sequence's paths cannot resolve, or null when that is not
 * what happened.
 *
 * Everything a sequence references — an executor's binary, a subsequence — is
 * named **relative to the sequence file** (ADR-0025, ADR-0027). A document from
 * File ▸ New has no file yet, so there is nothing for those paths to be
 * relative to, and the loader rejects it with a message that is true and
 * misleading at once: "its 'path' … does not exist" sends someone hunting for a
 * typo that is not there.
 *
 * `saved` is whether the document has a file on disk.
 */
export function unsavedPathHint(saved, message) {
  if (saved) return null;
  if (!/'path'|'sequence'|no existe|does not exist/.test(message)) return null;
  return "save the sequence first: what it references is relative to the file, and this one has no file yet";
}
