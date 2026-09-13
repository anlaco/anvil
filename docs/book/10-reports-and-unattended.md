# 10. Reports and running unattended

On a production line nobody reads the console. This chapter is about the
files Anvil leaves behind and the options for running without a person in
front of it.

## Reports

`--json <file>` and `--csv <file>` write the report to a file, in addition to
the console. `--quiet` removes the console report:

```console
$ anvil sequences/judging.yseq --quiet --csv judging.csv
ejecutor de pasos escuchando en 46037
motor conectado
catálogo pedido
conexión cerrada; esperando otra
$ cat judging.csv
sequence_name,status,step_name,step_status,message,measured_value,limit_min,limit_max,expected_value,operator,phase,inputs,outputs
judging,pass,board/measure_rail,pass,,4.98,4.75,5.25,,,main,,
judging,pass,board/measure_leakage,pass,,0.0004,,,0.001,<=,main,,
judging,pass,board/check_led,pass,,,,,,,main,,
```

One row per step. Unlike the console, the files carry the measurement and the
limit it was judged against: `measured_value` with `limit_min` and `limit_max`
for a range, `expected_value` and `operator` for a comparison. They also carry
the step's `inputs` and `outputs`, and a subsequence's steps are prefixed with
the name of their call. The JSON report has the same information, nested
(chapter 7 showed one).

Notice that `--quiet` did not silence the diagnostics on the error stream
([#35](https://github.com/anlaco/anvil/issues/35)). If a script needs silence,
redirect it.

## Changing limits without touching the sequence

A tolerance often depends on the product variant or the production lot, not on
the test. `--limits` loads a *limits file* that replaces the limits written in
the sequence, by step name. `sequences/judging.limits.yaml`:

```yaml
board/measure_rail:
  type: range
  min: 5.0
  max: 5.1
```

```console
$ anvil sequences/judging.yseq --limits sequences/judging.limits.yaml 2>/dev/null
=== judging: fail ===
  [fail] board/measure_rail: 4.98 fuera de rango [5, 5.1]
```

The same sequence, a tighter tolerance, a different verdict. The limits file
applies to every step with that name, in subsequences too.

A name that matches no step would leave the sequence's own limit in force
without anyone noticing, so Anvil warns — even under `--quiet`. With
`sequences/judging.typo.limits.yaml`:

```yaml
board/measure_rial:
  type: range
  min: 5.0
  max: 5.1
```

```console
$ anvil sequences/judging.yseq --limits sequences/judging.typo.limits.yaml
ejecutor de pasos escuchando en 40627
secuencia 'judging' cargada (3 pasos en main, 0 subsecuencia(s) externa(s), 1 ejecutor(es))
sidecar de límites 'sequences/judging.typo.limits.yaml' aplicado (0 paso(s) afectado(s))
aviso: 1 límite(s) del sidecar 'sequences/judging.typo.limits.yaml' no afectan a ningún paso: board/measure_rial
aviso: el sidecar no afectó a ningún paso. Comprueba que los nombres coincidan con los de los pasos de la secuencia
motor conectado
conectado a los ejecutores de pasos (embebido en 127.0.0.1:40627)
catálogo pedido
3 paso(s) comprobados contra el catálogo de su ejecutor
=== judging: pass ===
  [pass] board/measure_rail: 
  [pass] board/measure_leakage: 
  [pass] board/check_led: 
conexión cerrada; esperando otra
```

The `aviso` lines say that one limit in the file affects no step,
`board/measure_rial`, and that the file changed nothing: check the names. The
sequence passed against its own limit.

## Moving an executor without editing the sequence

`--executor name=host:port` points a declared executor somewhere else for this
run — the same sequence against the executor on your desk or on the line's PC:

```console
$ anvil sequences/first.yseq --executor bench=127.0.0.1:9201
```

The name must be one the sequence declares; any other is an error when the
file loads. It can be repeated, once per executor.

## Checking in CI

```console
$ anvil sequences/board.yseq --validate
secuencia 'board' cargada (4 pasos en main, 1 subsecuencia(s) externa(s), 1 ejecutor(es))
'board' válida (4 paso(s) en main, 1 subsecuencia(s) externa(s))
$ echo $?
0
```

`--validate` needs no executor and no bench, and exits with `0` when the file
is valid, so it can guard every change to a sequence in CI. Remember from
chapter 7 what it cannot see: whether steps and their inputs exist. For that,
`--validate --with-executors` with the executors running — and, in 0.5.0, with
[#70](https://github.com/anlaco/anvil/issues/70) in mind.

## Exit codes

`0` when the sequence passes. `1` for everything else: a `fail`, an `error`, a
file that does not load, a sequence that does not match its executors, an
executor that cannot be reached. A script can stop on the exit code, and read
the report to know which of those it was.

## Following a run as it happens

`--events` writes one JSON line per event — a step starting, a step's result —
to the error stream while the sequence runs. It is what the Sequence Editor
reads to light up the running step. `--quiet` does not silence it, and it
carries the same data as `--json`, instrument addresses included.

## Not covered here

`--process-model` wraps a sequence in a process model that identifies the unit
and reports on it. The only process model in 0.5.0 relies on the engine's
built-in demonstration steps, and it is not in the release archive, so this
book leaves it out.
