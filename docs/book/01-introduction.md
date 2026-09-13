# 1. Introduction

Anvil is a test sequencer: it runs a list of test steps against a unit on a
bench, judges what they measure and writes down what happened. If you have
used NI TestStand or OpenTAP, the job is the same one.

What makes Anvil different is where things live.

- **A sequence is a text file**, not a program. It lists the steps, the limits
  each measurement must fall within, and what to do before and after. You can
  read it in a diff, and changing a limit does not mean recompiling anything.
- **A step is ordinary code in your language** — C#, Python or Rust — served by
  an *executor*: a small process (or, for Rust, a module) that publishes the
  steps it knows how to run. Anvil calls each step by name.
- **The step measures; Anvil judges.** A step returns a number. Whether that
  number is acceptable is written in the sequence, and Anvil decides. The same
  step can serve two products with different tolerances without being touched.

## The words

| Word | What it means here |
|---|---|
| **sequence** | A `.yseq` (or `.yaml`) file: the steps to run and how to judge them. |
| **step** | One action or measurement, called by a name like `board/measure_rail`. |
| **executor** | The program that runs steps. A sequence says where each one is. |
| **module** | The first half of a step's name. In C# it comes from the class. |
| **limit** | The acceptance criterion for a measurement, written in the sequence. |
| **verdict** | What a step or a sequence ended as: `pass`, `fail`, `error` or `skipped`. |

The difference between `fail` and `error` matters more than anything else in
this book, and chapter 5 is about it: **`fail` says something about the unit,
`error` says something about the bench.** A unit that measures out of range
fails. An instrument that does not answer is an error — Anvil could not judge
the unit, so it does not pretend to.

## What this book covers

Writing steps in C#, writing sequences, running them from the command line and
reading the results. One chapter shows what changes when the step is written in
Python or Rust instead.

It does not cover talking to real instruments — every step here pretends to
measure — nor how Anvil is built inside. For that, see the
[documentation index](../README.md).

## What you need

- **Linux on x86-64.** Everything in the book was run there. Anvil also ships
  for Windows; the commands are the same with `.exe`, but this book has **not
  verified** them on Windows.
- **The .NET 10 SDK** (`dotnet --version` should print `10.` something).
- **git**, to get the step SDK.
- For chapter 12 only: Python 3.10 or newer, and a Rust toolchain.

No instrument, no licence and no network connection to a bench.
