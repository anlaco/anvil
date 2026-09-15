// Where the shell looks for the engine.
//
// The bug this pins down was a single hard-coded path: the shell only ever
// looked inside the checkout it was launched from, so an **installed** editor
// found nothing, Run stayed disabled, and the tooltip sent the person to a
// terminal to start `anvil --bridge` — whose URL a packaged window has no way
// to accept (#67). The order and the not-found message are asserted here
// because the machine that gets this wrong is the one nobody developing on a
// checkout has.

import { strict as assert } from "node:assert";
import { test } from "node:test";

import { anvilCandidates, anvilName, findAnvil, notFoundMessage } from "../electron/find-anvil.mjs";

const WIN = {
  platform: "win32",
  env: { PATH: "C:\\Windows;C:\\Program Files\\anvil" },
  execPath: "C:\\Users\\a\\AppData\\Local\\Programs\\anvil-editor\\Anvil Sequence Editor.exe",
  resourcesPath: "C:\\Users\\a\\AppData\\Local\\Programs\\anvil-editor\\resources",
  repo: "C:\\Users\\a\\AppData\\Local\\Programs\\anvil-editor\\resources",
};

const paths = (opts) => anvilCandidates(opts).map((c) => c.path);

test("the engine is named after the platform", () => {
  assert.equal(anvilName("win32"), "anvil.exe");
  assert.equal(anvilName("linux"), "anvil");
  assert.equal(anvilName("darwin"), "anvil");
});

test("an installed Windows editor looks beyond the dev tree", () => {
  // The whole point: none of these but the first existed before, and on an
  // installed editor the first is a directory that is not there.
  const where = paths(WIN);
  assert.ok(where.includes("C:\\Program Files\\anvil\\anvil.exe"), "PATH is searched");
  assert.ok(
    where.includes("C:\\Users\\a\\AppData\\Local\\Programs\\anvil-editor\\anvil.exe"),
    "next to the editor is searched",
  );
  assert.ok(
    where.includes("C:\\Users\\a\\AppData\\Local\\Programs\\anvil-editor\\resources\\anvil\\anvil.exe"),
    "the app's own resources are searched",
  );
});

test("what is said out loud wins over what is guessed", () => {
  const order = paths({
    ...WIN,
    env: { ...WIN.env, ANVIL_BIN: "D:\\bench\\anvil.exe" },
    remembered: "E:\\chosen\\anvil.exe",
  });
  assert.deepEqual(order.slice(0, 2), ["D:\\bench\\anvil.exe", "E:\\chosen\\anvil.exe"]);
});

test("a checkout runs the engine it just built", () => {
  // Before anything installed: `npm run app` exists to exercise this build,
  // and a release copy on PATH taking priority would hide the change under
  // test behind whatever was installed last.
  const order = paths({ platform: "linux", env: { PATH: "/usr/bin" }, repo: "/src/anvil" });
  assert.equal(order[0], "/src/anvil/packaging/anvil-host/target/release/anvil");
});

test("PATH is read the way each platform writes it", () => {
  const win = paths({ platform: "win32", env: { Path: '"C:\\tools";C:\\bin' } });
  assert.deepEqual(win, ["C:\\tools\\anvil.exe", "C:\\bin\\anvil.exe"]);

  const nix = paths({ platform: "linux", env: { PATH: "/usr/local/bin:/usr/bin:" } });
  assert.deepEqual(nix, ["/usr/local/bin/anvil", "/usr/bin/anvil"]);
});

test("one place is one place, however many ways it is reached", () => {
  const twice = paths({
    platform: "win32",
    env: { PATH: "C:\\Apps\\Editor" },
    execPath: "C:\\Apps\\Editor\\editor.exe",
  });
  assert.deepEqual(twice, ["C:\\Apps\\Editor\\anvil.exe"]);
});

test("the first one that is really there is the one taken", () => {
  const there = "C:\\Program Files\\anvil\\anvil.exe";
  const found = findAnvil({ ...WIN, exists: (p) => p === there });
  assert.equal(found.path, there);
  assert.equal(found.where, "PATH");

  assert.equal(findAnvil({ ...WIN, exists: () => false }), null);
});

test("not found says what to do and where it looked", () => {
  const message = notFoundMessage(anvilCandidates(WIN), "win32");
  // A bare "not found" is what makes someone reinstall what they already have.
  assert.match(message, /anvil\.exe/);
  assert.match(message, /releases/);
  assert.match(message, /ANVIL_BIN/);
  assert.match(message, /PATH/);
  assert.match(message, /C:\\Program Files\\anvil\\anvil\.exe/);
  // The first line is what reaches the Run button's tooltip.
  assert.equal(message.split("\n")[0], "could not find the engine (anvil.exe).");
});
