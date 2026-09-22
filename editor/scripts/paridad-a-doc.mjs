// Turns the parity inventory into `docs/paridad-teststand.md` (ADR-0043 §5).
//
// The inventory lives in `src/paridad.mjs` so that the greyed cells and this
// page cannot disagree. That only holds if nobody edits the page by hand, so
// the page is generated and CI runs this with `--check`: the build fails when
// the committed file and the inventory have drifted. Same arrangement
// `docs/book/check.sh` uses for the book's sessions.
//
//   node scripts/paridad-a-doc.mjs          # write it
//   node scripts/paridad-a-doc.mjs --check  # fail if it would change
//
// The page is in English because its content is the editor's own strings, and
// because it is what someone holding a TestStand licence reads before deciding
// whether any of this is worth their afternoon.

import { readFileSync, writeFileSync } from "node:fs";
import { argv } from "node:process";
import { fileURLToPath } from "node:url";

import { TESTSTAND, parityEntries } from "../src/paridad.mjs";

/** Where the page lives, relative to this script. */
export const OUT = fileURLToPath(new URL("../../docs/paridad-teststand.md", import.meta.url));

/** How each verdict is headed, and what the heading has to make clear. */
const SECTIONS = [
  {
    state: "todo",
    title: "Not yet",
    blurb:
      "Anvil intends to have these. Each one names the engine capability it is waiting on — so several rows naming the same capability are a priority, not a coincidence.",
    column: "Waiting on",
    field: "needs",
  },
  {
    state: "elsewhere",
    title: "Done another way",
    blurb:
      "Anvil solves these, differently. A missing cell here is not a missing feature; it is a feature that lives somewhere else.",
    column: "In Anvil",
    field: "anvil",
  },
  {
    state: "never",
    title: "Not planned",
    blurb:
      "Deliberately out of scope. Each one cites the decision that put it there, and changing one of these takes an ADR.",
    column: "Why not",
    field: "why",
  },
  {
    state: "built",
    title: "Built",
    blurb: "Anvil has these. They are listed so the inventory is a census rather than a list of holes.",
  },
];

/** Escapes what would otherwise break a Markdown table cell. */
const cell = (text) => String(text ?? "").replaceAll("|", "\\|").replaceAll("\n", " ");

export function render() {
  const entries = parityEntries();
  const areas = [...new Set(entries.map((e) => e.area))];
  const count = (state) => entries.filter((e) => e.state === state).length;

  const out = [];
  out.push("# Anvil against TestStand, feature by feature");
  out.push("");
  out.push(
    "**This page is generated** from `editor/src/paridad.mjs` by",
    "`editor/scripts/paridad-a-doc.mjs`. Do not edit it by hand: the same file",
    "decides what the Sequence Editor greys out, and the point of generating",
    "this is that the two cannot disagree. CI checks it.",
  );
  out.push("");
  out.push(
    `The reference is **NI TestStand ${TESTSTAND}**, and everything asserted here about`,
    "TestStand is **second-hand**: read from NI's documentation and from screenshots,",
    "not exercised in TestStand. The decision behind all of this is",
    "[ADR-0043](adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md);",
    "the editor's own rules are in [diseno/principios-del-editor.md](diseno/principios-del-editor.md).",
  );
  out.push("");
  out.push("## Where it stands");
  out.push("");
  out.push("| | Count |");
  out.push("|---|---:|");
  for (const { state, title } of SECTIONS) out.push(`| ${title} | ${count(state)} |`);
  out.push(`| **Total** | **${entries.length}** |`);
  out.push("");
  out.push(
    "A gap is never simply left off the interface: it appears where TestStand puts",
    "it, greyed, saying what TestStand does there and which of these it is. The",
    "engine is what unlocks a cell — a feature is built and verified headless, and",
    "only then does the editor stop greying it.",
  );
  out.push("");

  for (const section of SECTIONS) {
    const rows = entries.filter((e) => e.state === section.state);
    if (rows.length === 0) continue;
    out.push(`## ${section.title}`);
    out.push("");
    out.push(section.blurb);
    out.push("");
    for (const area of areas) {
      const inArea = rows.filter((e) => e.area === area);
      if (inArea.length === 0) continue;
      out.push(`### ${area}`);
      out.push("");
      if (section.field) {
        out.push(`| | In TestStand | ${section.column} |`);
        out.push("|---|---|---|");
        for (const e of inArea) {
          out.push(`| **${cell(e.label)}** | ${cell(e.teststand)} | ${cell(e[section.field])} |`);
        }
      } else {
        out.push("| | |");
        out.push("|---|---|");
        for (const e of inArea) {
          out.push(`| **${cell(e.label)}** | \`${cell(e.id)}\` |`);
        }
      }
      out.push("");
    }
  }

  return out.join("\n").replace(/\n{3,}/g, "\n\n").trimEnd() + "\n";
}

/** Writing and checking are the same rendering, so they cannot diverge. */
function main() {
  const wanted = render();

  if (!argv.includes("--check")) {
    writeFileSync(OUT, wanted);
    console.log(`paridad: wrote ${OUT}`);
    return 0;
  }

  let have = null;
  try {
    // Line endings normalised away: this asks whether the page still says what
    // the inventory says, and a checkout that writes CRLF is not an answer to
    // that. It failed on `windows-latest` for exactly that reason, reporting a
    // diff in which every line was identical.
    have = readFileSync(OUT, "utf8").replace(/\r\n/g, "\n");
  } catch {
    console.error(`paridad: ${OUT} does not exist. Run: node editor/scripts/paridad-a-doc.mjs`);
    return 1;
  }
  if (have !== wanted) {
    console.error(
      "paridad: docs/paridad-teststand.md does not match editor/src/paridad.mjs.\n" +
        "  The inventory moved and the page did not. Run:\n" +
        "    node editor/scripts/paridad-a-doc.mjs",
    );
    return 1;
  }
  console.log("paridad: docs/paridad-teststand.md is up to date");
  return 0;
}

// Only when run, never when imported: the test reads `render` and `OUT`.
if (fileURLToPath(import.meta.url) === argv[1]) process.exit(main());
