// Guards the promise of ADR-0031: the editor and the command line must carry
// the same engine.
//
// They share a source artefact but not necessarily the same compilation of it.
// `packaging/anvil-host/build.rs:37` takes the guest from the profile the host
// is built with, while the editor transpiles `release` unconditionally. So
// after `make build`, the binary carries a guest built from the current source
// and the editor holds one built from whatever the source was last time — and
// nothing says so. The symptom is the editor and the CLI disagreeing about the
// same file, which is among the most expensive things to diagnose, so this is
// checked at the one point a stale guest gets in: transpiling.
//
// **The question is whether the release guest is older than the engine's
// source**, not whether some other profile is newer. The first version of this
// compared the release and debug artefacts, and its first run was a false
// positive: the debug guest was a day newer, but no commit in that window
// touched `crates/motor` or anything it depends on, so the two were the same
// engine. Comparing against the source catches the case that matters — someone
// edited the engine and did not rebuild release — and also one the artefact
// comparison missed entirely, which is editing the engine and building nothing.
//
// Split out from transpile.mjs and taking plain values so it can be tested
// without building anything.

/** The crates the engine guest is built from; a change to any is a change to it. */
export const ENGINE_SOURCES = [
  "crates/motor",
  "crates/cargador",
  "crates/expr",
  "crates/modelo",
  "crates/result_sink",
];

/**
 * Whether the release guest the editor is about to transpile is behind the
 * engine's source, and what to say about it.
 *
 * `guestMtime` is epoch milliseconds, or null when it has not been built.
 * `sourceMtime` is the most recent modification across `ENGINE_SOURCES`.
 * Returns null when there is nothing to report.
 */
export function checkFreshness({ guestMtime, sourceMtime }) {
  if (guestMtime === null) {
    return {
      fatal: true,
      message:
        "the release engine guest does not exist.\n" +
        "The editor hosts the same engine the binary embeds (ADR-0031).\n" +
        "Build it first:  make release",
    };
  }

  // "I could not check" is not "there is nothing to report" (ADR-0019, Rule 2).
  // This guard's first version returned null here, and a bug that made every
  // source scan throw then passed silently for exactly that reason.
  if (sourceMtime === null || Number.isNaN(sourceMtime)) {
    return {
      fatal: true,
      message:
        "could not determine when the engine's source last changed, so whether\n" +
        "the editor and the binary carry the same engine is unknown (ADR-0031).\n" +
        "This is reported rather than assumed to be fine.\n\n" +
        "Transpile anyway:  npm run transpile -- --allow-stale",
    };
  }

  if (sourceMtime > guestMtime) {
    const behind = Math.round((sourceMtime - guestMtime) / 1000);
    return {
      fatal: true,
      message:
        `the engine's source changed ${humanise(behind)} after the release guest was built.\n` +
        "The editor would host an engine older than the one the binary would,\n" +
        "and they must not differ (ADR-0031): the two would then disagree about\n" +
        "the same sequence, with nothing saying why.\n\n" +
        "Fix it:            make release\n" +
        "Transpile anyway:  npm run transpile -- --allow-stale",
    };
  }

  return null;
}

function humanise(seconds) {
  if (seconds < 90) return `${seconds}s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)}min`;
  if (seconds < 172800) return `${Math.round(seconds / 3600)}h`;
  return `${Math.round(seconds / 86400)} days`;
}
