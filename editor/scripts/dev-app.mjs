// Runs the editor the way a person downloads it: the SPA served by Vite,
// inside the Electron shell (ADR-0037). Two processes with an ordering
// between them — the window loads `http://localhost:5180`, so Vite has to be
// answering before Electron opens it, or the window shows a connection
// error and never retries.
//
//   npm run app                                   # just open it
//   ANVIL_EDITOR_OPEN=/ejemplos/basica.yseq npm run app   # with a sequence
//   ANVIL_EDITOR_DEBUG_PORT=9222 npm run app      # inspectable over CDP
import { spawn } from "node:child_process";
import { statSync } from "node:fs";
import path from "node:path";
import electron from "electron";

const URL = "http://localhost:5180";

// Chromium refuses to start unsandboxed, and on Linux it needs one of two
// things this repo cannot provide from inside `npm install`: a `chrome-sandbox`
// helper owned by root with the setuid bit, or unprivileged user namespaces —
// which Ubuntu 23.10+ and derivatives restrict through AppArmor
// (`kernel.apparmor_restrict_unprivileged_userns=1`). Rather than quietly pass
// `--no-sandbox`, which is a security decision and not a detail, this says what
// is wrong and makes the workaround something a person opts into out loud.
function sandboxArgs() {
  if (process.platform !== "linux") return [];
  if (process.env.ANVIL_EDITOR_NO_SANDBOX === "1") {
    console.warn("! running Chromium with --no-sandbox (ANVIL_EDITOR_NO_SANDBOX=1)");
    return ["--no-sandbox"];
  }
  const helper = path.join(path.dirname(electron), "chrome-sandbox");
  try {
    const st = statSync(helper);
    // setuid bit (04000) and owned by root: the SUID sandbox's requirement.
    if (st.uid === 0 && st.mode & 0o4000) return [];
  } catch {
    return [];
  }
  console.error(
    [
      "Chromium's sandbox helper is not set up, so the window will not open.",
      "",
      "  Fix it once (recommended, keeps the sandbox on):",
      `    sudo chown root:root ${helper}`,
      `    sudo chmod 4755 ${helper}`,
      "",
      "  Or skip the sandbox for this run (development only):",
      "    ANVIL_EDITOR_NO_SANDBOX=1 npm run app",
    ].join("\n"),
  );
  process.exit(1);
}

const extra = sandboxArgs();

const vite = spawn("npx", ["vite"], { stdio: "inherit" });

async function waitForVite(deadlineMs = 30_000) {
  const until = Date.now() + deadlineMs;
  while (Date.now() < until) {
    try {
      await fetch(URL);
      return true;
    } catch {
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  return false;
}

if (!(await waitForVite())) {
  console.error(`vite did not answer on ${URL} within 30s`);
  vite.kill();
  process.exit(1);
}

const app = spawn(electron, [".", ...extra], { stdio: "inherit" });

// Neither outlives the other: closing the window stops the dev server, and
// a dev server that dies takes the window with it.
const stop = (code) => {
  vite.kill();
  app.kill();
  process.exit(code ?? 0);
};
app.on("exit", stop);
vite.on("exit", stop);
process.on("SIGINT", () => stop(0));
