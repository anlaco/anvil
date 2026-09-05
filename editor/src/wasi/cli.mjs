// The browser build of the WASI shim, used everywhere — including Node.
//
// `@bytecodealliance/preview2-shim` resolves to a Node-specific build under
// Node's `node` export condition, and that build routes stdio through an I/O
// worker whose streams cannot be swapped for a plain capture object. The
// browser build takes a `{ write }` handler directly and is what actually
// ships; testing against the Node build would be testing something we never run.
//
// Reached by file path because the package's `exports` map admits no subpath
// that names the browser build: `./*` resolves to `dist/nodejs/*.js` under Node
// and there is no condition to override it from the importing side.

export * from "../../node_modules/@bytecodealliance/preview2-shim/dist/browser/cli.js";
