# The Anvil Book

Learn to use Anvil from nothing: install it, write test steps, put them in
sequences, run them, and read what Anvil says back.

> **Alpha.** This edition is written and checked against **Anvil 0.5.0 on
> Linux x86-64**; installing on Windows is covered and checked too. Anvil is young, and some of what you will do here — cloning
> the repository to get the step SDK, above all — will get shorter. Where 0.5.0
> has a rough edge or a known defect, the book says so where you meet it,
> rather than pretending it is not there.

## Contents

1. [Introduction](01-introduction.md) — what Anvil is, the words it uses, and
   what you need.
2. [Installing](02-installing.md) — the engine, the step SDK, your working
   folder.
3. [Your first step](03-first-step.md) — a C# method the engine can call.
4. [Your first sequence](04-first-sequence.md) — the file that says what to
   run, and how to run it.
5. [Measuring and judging](05-judging.md) — limits, pass, fail and error.
6. [Setup, main, cleanup and retries](06-phases-and-retries.md)
7. [Inputs, variables and flow](07-data.md)
8. [Instruments that stay open](08-references.md) — references to objects
   that live in the executor.
9. [Subsequences](09-subsequences.md)
10. [Reports and running unattended](10-reports-and-unattended.md)
11. The Sequence Editor — *not written yet*. In 0.5.0 the packaged editor
    opens, but it cannot run a sequence
    ([#80](https://github.com/anlaco/anvil/issues/80)).
12. [Steps in Python and Rust](12-other-languages.md)
13. [When something goes wrong](13-troubleshooting.md)

Read chapters 1 to 4 in order. After that each chapter stands mostly on its
own, although each one adds a file or two to the same C# project.

## Conventions

Everything happens in one folder, `~/anvil-book`. Commands start with `$` and
are run from that folder, unless the text says otherwise. The lines after a
command are what it printed — pasted from a real run, not retyped.

Anvil's own diagnostic messages are still partly in Spanish in 0.5.0
([#59](https://github.com/anlaco/anvil/issues/59)). The book shows them as
they are printed and explains what they say.

## How this book stays true

Every command in these chapters has been run against the release named above,
and none of the output was written by hand. The files you are asked to write
live in [`listings/`](https://github.com/anlaco/anvil/tree/main/docs/book/listings), the terminal sessions in
[`listings/sessions/`](https://github.com/anlaco/anvil/tree/main/docs/book/listings/sessions), and [`check.sh`](https://github.com/anlaco/anvil/blob/main/docs/book/check.sh) runs
every session again and checks that each file still appears word for word in
a chapter:

```console
$ docs/book/check.sh path/to/anvil-v0.7.0-x86_64-linux-musl
```

What could not be run is marked **not verified**, with those words.
