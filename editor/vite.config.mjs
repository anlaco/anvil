import { readFile } from "node:fs/promises";
import { join, normalize, resolve } from "node:path";
import { defineConfig } from "vite";

const REPO = resolve(import.meta.dirname, "..");

// Serves the repo's own `ejemplos/` in dev, so `?open=/ejemplos/basica.yaml`
// works without a file picker. Dev only — it is how the editor is exercised
// against the real fixtures, and the browser's file dialog cannot be driven
// from a test.
const examples = {
  name: "anvil-examples",
  apply: "serve",
  configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if (!req.url?.startsWith("/ejemplos/")) return next();
      // Contain the path to `ejemplos/`: a dev server that will read any file
      // the process can reach is a dev server that leaks the machine.
      const rel = normalize(decodeURIComponent(req.url.split("?")[0])).replace(/^(\.\.[/\\])+/, "");
      const path = join(REPO, rel);
      if (!path.startsWith(join(REPO, "ejemplos"))) {
        res.statusCode = 403;
        return res.end("outside ejemplos/");
      }
      try {
        res.setHeader("content-type", "application/yaml; charset=utf-8");
        res.end(await readFile(path));
      } catch {
        res.statusCode = 404;
        res.end("not found");
      }
    });
  },
};

export default defineConfig({
  plugins: [examples],
  server: {
    port: 5180,
    // The engine is a 1.2 MB component transpiled into a 400 kB module; letting
    // Vite pre-bundle it costs a long pause on every cold start and buys
    // nothing, since it is already a single generated file.
    fs: { strict: false },
  },
  optimizeDeps: {
    exclude: ["../generated/anvil.js"],
  },
  build: {
    target: "es2022",
    // The generated core modules are fetched at runtime by name, so they have
    // to survive the build as files rather than being inlined or hashed.
    assetsInlineLimit: 0,
  },
});
