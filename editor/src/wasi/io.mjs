// The browser build of the WASI shim. See ./cli.mjs for why this reaches into
// node_modules by path instead of importing the package by name.

export * from "../../node_modules/@bytecodealliance/preview2-shim/dist/browser/io.js";
