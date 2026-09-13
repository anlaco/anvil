# 9. Subsequences

When the same few steps appear in several places, move them into a
subsequence and call it. A subsequence can live in the same file, or in a file
of its own that several sequences share.

This chapter needs no new steps.

## A file to share

`sequences/rail-check.yseq` measures one channel and hands the value back to
whoever called it:

```yaml
name: rail_check

parameters:
  channel: 1
  volts: 0.0

locals:
  measured: 0.0

main:
  - name: dmm/measure_voltage
    executor: bench
    inputs: { channel: '${parameters.channel}' }
    assign:
      measured: result.measured_value

  - name: return_volts
    type: statement
    statement: 'parameters.volts = locals.measured'
```

- **`parameters`** are what the caller passes in, with default values.
- It has **no `executors:` section**. The executors are declared once, in the
  sequence you run, and subsequences refer to them by name. Anvil refuses a
  subsequence that declares its own. It also means this file cannot be run on
  its own.
- **Returning a value takes two steps.** `assign` always writes to a local, so
  the measurement goes to `locals.measured`, and a `statement` copies it to the
  parameter the caller will read. Assigning directly to a parameter is refused
  when the file loads.

## Calling it

```yaml
name: board

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

locals:
  ch_3v3: 3
  v_3v3: 0.0
  ch_1v1: 1
  v_1v1: 0.0

subsequences:
  power_up:
    setup:
      - name: fixture/power_on
        executor: bench
    main:
      - name: board/check_led
        executor: bench

main:
  - name: power_up
    type: sequence_call
    sequence: power_up

  - name: measure_3v3
    type: sequence_call
    sequence: ./rail-check.yseq
    args: { channel: locals.ch_3v3, volts: locals.v_3v3 }

  - name: measure_1v1
    type: sequence_call
    sequence: ./rail-check.yseq
    args: { channel: locals.ch_1v1, volts: locals.v_1v1 }

  - name: rails_in_spec
    type: pass_fail
    condition: 'locals.v_3v3 > 3.2 && locals.v_3v3 < 3.4 && locals.v_1v1 > 1.0 && locals.v_1v1 < 1.2'

cleanup:
  - name: fixture/power_off
    executor: bench
```

```console
$ anvil sequences/board.yseq 2>/dev/null
=== board: pass ===
  [pass] power_up: sequence call 'power_up' → pass
    [pass] fixture/power_on: 
    [pass] board/check_led: 
  [pass] measure_3v3: sequence call 'sequences/rail-check.yseq' → pass
    [pass] dmm/measure_voltage: range auto
    [pass] return_volts: statement ok
  [pass] measure_1v1: sequence call 'sequences/rail-check.yseq' → pass
    [pass] dmm/measure_voltage: range auto
    [pass] return_volts: statement ok
  [pass] rails_in_spec: condición cumplida
  [pass] fixture/power_off: 
```

- **`subsequences`** holds `power_up`, a subsequence that lives in this file. It
  has its own `setup` and `main`.
- A step with **`type: sequence_call`** calls one. `sequence` is either the name
  of a subsequence in the same file (`power_up`) or a path to a file, relative
  to the file doing the calling (`./rail-check.yseq`).
- **`args`** connects each parameter to one of the caller's locals —
  `channel: locals.ch_3v3`. It is written without `${...}` because it is not a
  value: it names the variable, and when the subsequence writes the parameter,
  the caller's local changes. That is how `locals.v_3v3` gets its value.

The report nests each subsequence's steps under the call — indented on the
console, under `sub_steps` in the JSON report — and the call takes the verdict
of the subsequence: if a step inside errors, the call is an `error` too, and
the calling sequence stops as it would for any other step.
