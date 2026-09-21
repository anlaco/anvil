# ADR-0045: Terminating a run is a request, checked between steps

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-21
- **How it was decided:** in this repo, working down the queue ADR-0043 made
  visible. Building the editor's execution window put five controls on screen
  greyed — Break, Resume, Step, Terminate, Abort — and four of them turned out
  to wait on one missing piece. This ADR is the first of it, and the one that
  was already a risk before any of them were drawn: `editor/README.md` has said
  since the editor could run anything that *"killing the thread mid-sequence
  leaves the bench exactly as it was, with no `cleanup` run"*. Everything
  asserted here about this repo was verified by reading the code in this
  session and is cited with file and line.
- **Relates to:** ADR-0011, ADR-0019, ADR-0029, ADR-0031, ADR-0037, ADR-0040,
  ADR-0043, [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md),
  [ui-vs-headless.md](../diseno/ui-vs-headless.md)
- **Scope:** decides **what it means to stop a run that is in flight**, where
  the engine may notice, what still has to happen when it does, and what the
  run then reports. It does **not** add a step status, does **not** change
  `paso.proto` or `contract`, and does **not** implement anything. It is
  **not** breakpoints: stopping *at* a step and resuming is a different piece
  and needs its own decision. It does **not** decide the editor's controls
  beyond which one this unlocks.

## Context

### 1. There is no cancellation anywhere

`grep` over `crates/` and `packaging/` finds no cancel, no abort, no signal
handling. The engine runs a sequence from `ejecuta_programa` to the end and the
only way to stop it is to kill the process. In the editor that is literal:
`engine-pool.mjs:194-196` is `terminateAll()`, which calls `worker.terminate()`
— the thread stops wherever it was.

### 2. The one thing that must not happen, happens

A sequence's shape already guarantees a bench is left safe. In
`ejecuta_secuencia_interna` the Setup loop breaks on failure, the Main loop
breaks on the first failure, and the Cleanup loop **runs regardless**; and
because the function recurses for a `sequence_call`, that holds at every depth.
`cleanup` is where the power supply is switched off and the fixture is opened.

Killing the thread walks past all of it. The engine is stopped between one
instruction and the next, and nothing in that guarantee applies to a process
that is no longer there. `editor/README.md` calls it *"survivable while nothing
is executed — a validate touches only memory"*, and it is not survivable now
that Run reaches real hardware.

### 3. Four greyed controls, one missing piece — and this is not it

The execution toolbar of ADR-0043 shows Break, Resume, Step Into/Over/Out,
Terminate and Abort. Break, Resume and Step wait on **stopping at a step and
resuming**, which is a larger thing: it needs the engine to hold a run open,
and something to inspect while it is held.

Terminate does not. It needs the engine to notice that it has been asked to
stop and then take the exit it already knows how to take. That is why it is
first: it is the one whose absence is dangerous rather than merely missing, and
it is the one the existing structure almost implements.

## Decision

### 1. Terminate is a request, and it is honoured between steps

The engine checks for it **before invoking each step** of `setup` and `main`,
and **never** inside an invocation. A step that has reached the instrument
finishes: Anvil does not know what it started, whether a relay is half thrown
or a supply is ramping, and interrupting the executor mid-call would leave the
bench in a state nobody can name. Between steps is the only place where what
the bench is doing is known.

The cost is stated plainly: **terminating does not stop the bench instantly.**
It stops it at the next boundary, and a step that takes a minute takes a minute.
An immediate stop is a different, physical thing — an E-stop — and no software
request should be mistaken for one.

### 2. Cleanup runs, at every level of the stack

A terminated run does not skip `cleanup`; that is the whole point. Setup and
Main stop as though the phase had ended, Cleanup runs, and a `sequence_call`
unwinds through each caller's Cleanup in turn — the deepest first, which is the
order the bench was built up in, reversed.

This is not new machinery. It is what `ejecuta_secuencia_interna` does when
Main's first step fails, and a termination takes the same exit.

**Cleanup is not itself terminable.** A second request while Cleanup is running
is recorded and ignored: a Cleanup that can be cut short is not a guarantee,
and whoever is pressing the button twice is asking for exactly the thing this
ADR exists to prevent. What remains for the person who truly must stop now is
killing the process, which is honest about what it does.

### 3. The request arrives on **stdin**

One byte on the engine's standard input. Verified: the engine never reads
stdin — `grep` over `crates/motor` and `crates/cargador` finds no use of it —
so the channel is free, and it is the one channel both hosts already have.

- **Native** (ADR-0011): the host builds the guest's `WasiCtx` with
  `inherit_stdio()` (`packaging/anvil-host/src/main.rs:73`). It stops
  inheriting stdin, keeps the write end, catches `SIGINT`, and writes the byte.
  Ctrl-C then means *terminate*, which is what it should have meant all along —
  today it kills the host and the guest with it.
- **In the editor** (ADR-0037): the shim already provides the guest's WASI, and
  `engine.mjs` already hands it stdout and stderr handlers
  (`cliShim._setStdout`). stdin joins them, backed by the `SharedArrayBuffer`
  the network worker already shares, so the page's Terminate button sets a flag
  the engine worker can see without a message it is not waiting for.

The alternatives are worse in ways worth recording. **wasmtime's epoch
interruption** stops the guest where it stands — that is the thread-kill again,
with better manners. **A control socket** is a second protocol, a second port
and a second thing to secure, for one bit. **A file the engine stats** works
and needs a writable path the sandbox can see, which is a preopen granted for
nothing else.

### 4. A terminated run is not a pass, and needs no new status

This is the trap. A run terminated after three passing steps aggregates to
`pass` and exits 0 unless something stops it: a green light on a unit nobody
finished testing, which is Rule 1 of ADR-0019 exactly.

So a terminated run reports **`inconclusive`** — the status that already exists
for *"the sequence declared a verdict and none was evaluated"* (ADR-0019), and
already exits 1. The meaning fits without stretching: nobody judged the unit.
No new status, no new column, no `contract` move, and no third-party executor
learns a word — an executor cannot return this, the engine mints it, like
`done`.

What distinguishes a termination from any other inconclusive is the **reason**,
which belongs in the report next to the verdict rather than inside it. The
event stream says so too, at the end (ADR-0029): a reader that lost the tail of
a run and a reader watching one that was stopped must not see the same thing.

### 5. There is no Abort

TestStand has both: Terminate runs cleanup, Abort does not. Anvil has one.

An Abort that is honest is a killed process, and that is already available from
outside — the shell, the window's close button, the task manager. Putting a
button on it inside the editor would dress up *"leave the bench however it
is"* as a supported operation, and the one place it is genuinely needed is the
one place it should be deliberate and unaided.

The greyed **Abort** cell of ADR-0043 therefore becomes `never`, with this as
the reason — not `todo`.

## Consequences

a. **The editor's Terminate becomes buildable**, and the sentence in
   `editor/README.md` stops being true. That sentence is the reason this is
   first in the queue.

b. **Ctrl-C changes meaning at the command line.** Today it kills the host and
   the guest with it, taking the bench with them. After this it asks, and the
   sequence comes down through its own Cleanup. That is a behaviour change for
   anyone who has been using Ctrl-C to stop a run, and it is an improvement
   they did not ask for and will notice: the run takes a moment longer to end.
   A second Ctrl-C still kills, because the terminal's own escape hatch may not
   be taken away.

c. **`--quiet` does not silence it.** That a run was terminated is not the
   report; it is what happened, and it goes out like the other warnings do.

d. **It does not make the engine interruptible**, and nothing here should be
   read as a step towards it. The engine stays single-threaded and synchronous
   (`crates/motor/src` has no `thread` or `spawn`). What changes is that it
   looks at one byte between steps.

e. **Breakpoints are still not designed.** They need the engine to *hold* —
   to stop and stay stopped with its state readable — which is a different
   problem from noticing a request and taking an exit it already has. Three of
   the five greyed controls still wait, and this ADR does not shorten that
   wait; it removes the dangerous one from the list.

f. **Not decided here:** what a terminated run's JSON and CSV carry beyond the
   verdict, whether a `sequence_call` should be able to decline termination
   (it should not, but nothing here forbids writing that later), and what
   happens to a run terminated while an executor is being connected to rather
   than invoked.
