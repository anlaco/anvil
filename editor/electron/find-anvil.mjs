// Where the engine is, seen from the editor's shell.
//
// The shell starts `anvil <sequence> --bridge` itself rather than asking for a
// second terminal (ADR-0037 §d), and for a long time it looked in exactly one
// place: `packaging/anvil-host/target/release/` inside the checkout it was
// launched from. That path exists only in a dev tree, so on an **installed**
// editor there was nothing there — and because the engine is still a separate
// download (#67), Run was refused with `spawn … ENOENT` in the status bar and
// a tooltip telling the person to start `anvil --bridge` by hand, which the
// packaged window has no way to connect to.
//
// So the binary is looked for where it can actually be, in order, and when it
// is nowhere the shell asks for it rather than failing.
//
// This file touches neither `electron` nor the filesystem: everything it looks
// at arrives as an argument, which is what lets the order below be asserted
// (`editor/test/find-anvil.test.mjs`) instead of discovered on a bench.

import path from "node:path";

// Joined the way the *target* platform spells paths, not the way the machine
// running this does. It is the same object under Electron — `node:path` is
// already `win32` on Windows — and it is what lets a Linux test assert what a
// Windows install will look at.
const on = (platform) => (platform === "win32" ? path.win32 : path.posix);

/** What the engine's executable is called on this platform. */
export function anvilName(platform) {
  return platform === "win32" ? "anvil.exe" : "anvil";
}

/**
 * Every place the engine may be, most deliberate first.
 *
 * `where` is the half a person needs when none of them hold it: a list of
 * paths says nothing about which one they were supposed to fill.
 *
 * @returns {{path: string, where: string}[]}
 */
export function anvilCandidates({
  platform,
  env = {},
  execPath = null,
  resourcesPath = null,
  repo = null,
  remembered = null,
}) {
  const exe = anvilName(platform);
  const p = on(platform);
  const out = [];
  const add = (file, where) => {
    if (file) out.push({ path: p.normalize(file), where });
  };

  // Said out loud, so it wins: a bench with two engines on it is a bench where
  // guessing is the wrong thing to do.
  add(env.ANVIL_BIN, "ANVIL_BIN");
  // What was chosen through "Locate…" last time. Second because ANVIL_BIN is
  // the deliberate override of a remembered answer that has gone stale.
  add(remembered, "the engine you chose before");
  // The dev tree. Before the installed places so that a checkout runs what it
  // just built, which is the whole point of `npm run app`.
  if (repo) add(p.join(repo, "packaging", "anvil-host", "target", "release", exe), "the dev tree");
  // Dropped next to the installed editor, and the bundle this will become once
  // the engine ships with the editor (#67).
  if (resourcesPath) add(p.join(resourcesPath, "anvil", exe), "the app's resources");
  if (execPath) add(p.join(p.dirname(execPath), exe), "next to the editor");
  for (const dir of pathDirs(platform, env)) add(p.join(dir, exe), "PATH");

  // A path reached twice — `anvil` on PATH *and* next to the editor — is one
  // place to a person, and two lines in the message that says where it looked.
  const seen = new Set();
  return out.filter((c) => {
    const key = platform === "win32" ? c.path.toLowerCase() : c.path;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function pathDirs(platform, env) {
  const raw = env.PATH ?? env.Path ?? "";
  return raw
    .split(platform === "win32" ? ";" : ":")
    // `C:\Program Files\x` is quoted often enough on Windows that an unquoted
    // lookup would miss exactly the directory an installer wrote.
    .map((d) => d.trim().replace(/^"(.*)"$/, "$1"))
    .filter(Boolean);
}

/**
 * The first candidate that is really there, or `null`.
 *
 * `exists` is passed in rather than imported so the order can be tested
 * against a machine that has none of these paths.
 */
export function findAnvil({ exists, ...where }) {
  return anvilCandidates(where).find((c) => exists(c.path)) ?? null;
}

/**
 * What to say when it is nowhere — the message a person at a bench reads.
 *
 * It names the download, the two ways to point at an engine that is already
 * installed, and every path it tried, because "not found" without the list is
 * what makes someone reinstall the thing they already have.
 */
export function notFoundMessage(candidates, platform) {
  const exe = anvilName(platform);
  const looked = candidates.map((c) => `  ${c.path}  (${c.where})`).join("\n");
  return [
    `could not find the engine (${exe}).`,
    "",
    "The editor starts it to run a sequence, and it is still a separate",
    "download from the editor: take the archive for this platform from",
    "https://github.com/anlaco/anvil/releases and either put it on PATH, set",
    "ANVIL_BIN to its full path, or point at it with Locate….",
    "",
    "Looked in:",
    looked,
  ].join("\n");
}
