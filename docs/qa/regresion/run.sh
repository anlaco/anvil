#!/bin/bash
# Regression suite for the defects of the August 2026 beta.
# See docs/qa/informe-beta-2026-08.md
#
# Each case ASSERTS THE CORRECT BEHAVIOUR, so while the defect is still present
# it prints FALLA. Once fixed, it prints OK.
#
# Usage (from the repo root):
#   ./docs/qa/regresion/run.sh
#
# Needs the host binary built:
#   make release      (or `make build`, but it starts much more slowly)

cd "$(dirname "$0")/../../.." || exit 1

A=""
for cand in packaging/anvil-host/target/release/anvil \
            packaging/anvil-host/target/debug/anvil; do
  [ -x "$cand" ] && { A="$cand"; break; }
done
if [ -z "$A" ]; then
  echo "host binary not found. Build it with:" >&2
  echo "  make release" >&2
  exit 2
fi
echo "binary: $A"

R=docs/qa/regresion
ok=0; falla=0
# The binary only preopens the CWD: --csv/--json output must land inside the
# tree, not in /tmp.
TMP=$R/.tmp; mkdir -p "$TMP"; trap 'rm -rf "$TMP"' EXIT

check() {  # check <id> <description> <0=ok|1=fail>
  if [ "$3" -eq 0 ]; then
    printf '  \033[32mOK   \033[0m %-8s %s\n' "$1" "$2"; ok=$((ok+1))
  else
    printf '  \033[31mFALLA\033[0m %-8s %s\n' "$1" "$2"; falla=$((falla+1))
  fi
}

echo "== Beta 2026-08 regression ========================================"

# ---- DEF-1: --limits must reach the operator's sequence under a PM ----
$A --process-model $R/pm-minimal.yaml \
   ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml \
   >"$TMP/d1.out" 2>"$TMP/d1.err"
afectados=$(grep -oE 'aplicado \([0-9]+ paso' "$TMP/d1.err" | grep -oE '[0-9]+' | head -1)
grep -q ': pass ===' "$TMP/d1.out"; agregado=$?
[ "${afectados:-0}" -ge 1 ] && [ "$agregado" -eq 0 ]
check DEF-1 "sidecar applied under --process-model (affected=${afectados:-0}, expected>=1)" $?

# ---- DEF-2: the CSV's sequence-name column must carry the name ----
$A "$R/bug2-csv-nombre.yaml" --csv "$TMP/d2.csv" >/dev/null 2>&1
col1=$(sed -n '2p' "$TMP/d2.csv" 2>/dev/null | cut -d, -f1)
[ "$col1" = "regresion_csv_nombre" ]
check DEF-2 "CSV column 1 = sequence name (got: '${col1}')" $?

# ---- DEF-3a / DEF-3b: assign must not silently shadow a parameter ----
# The defect was closed the hard way —the loader refuses the sequence— so both
# cases accept the refusal as the fix. What they do **not** accept is nothing
# at all: without the guard below, a binary that printed not a single line
# came out green, because the `grep` for the failure mark did not match an
# empty file. DEF-3b passed exactly that way.
asigna_no_ensombrece() {  # <stdout> <stderr>
  # The refusal counts only if it is *this* refusal: any other load error (an
  # executor these files do not declare, say) used to satisfy a bare
  # 'inválida' and turned the case green for a reason unrelated to the shadow.
  if grep -qiE 'ensombrece' "$2"; then
    return 0            # the loader refuses it: an acceptable fix
  fi
  if ! grep -q '===' "$1"; then
    return 1            # neither refusal nor run: nothing was checked here
  fi
  grep -qE '\[skipped\] verificar_led' "$1" && return 1 || return 0
}

$A "$R/bug3-sub-asigna-parameter.yaml" >"$TMP/d3a.out" 2>"$TMP/d3a.err"
asigna_no_ensombrece "$TMP/d3a.out" "$TMP/d3a.err"
check DEF-3a "assign does not shadow a declared parameter" $?

# The same shadow seen from the parent: the by-reference return must not bring
# back the initial value instead of the measured one.
$A "$R/bug3-padre-asigna-parameter.yaml" >"$TMP/d3b.out" 2>"$TMP/d3b.err"
asigna_no_ensombrece "$TMP/d3b.out" "$TMP/d3b.err"
check DEF-3b "by-reference return does not bring the unmeasured value" $?

# ---- DIAG-1: warn when the sidecar affects no step ----
# This case used to reuse DEF-1's command, which gave 0 affected **because of
# the defect**. With DEF-1 fixed, a genuinely orphan sidecar is needed.
cat >"$TMP/huerfano.limits.yaml" <<'YAML'
paso_que_no_existe:
  comparison: GE
  low: 4.0
YAML
$A ejemplos/limites.yaml --limits "$TMP/huerfano.limits.yaml" 2>&1 |
  grep -qiE 'aviso.*sidecar|sidecar.*no afect|ningún paso'
check DIAG-1 "warning when the sidecar affects 0 steps" $?

# ---- DIAG-3: the report must say which phase each step ran in ----
# Without it, post-processing cannot tell a Setup failure (the DUT could not
# even be connected) from a Main or Cleanup one.
$A ejemplos/basica.yaml --json "$TMP/d3.json" --csv "$TMP/d3.csv" >/dev/null 2>&1
res=0
for f in setup main cleanup; do
  grep -q "\"phase\": \"$f\"" "$TMP/d3.json" || res=1
done
# The CSV is CRLF (RFC-4180): the \r has to go before looking.
# The column is not anchored at the end: what this case asserts is that the
# phase is there, not where — `inputs` and `outputs` were added later
# (ADR-0020) and left it in the middle.
head -1 "$TMP/d3.csv" 2>/dev/null | tr -d '\r' | grep -qE '(^|,)phase(,|$)' || res=1
check DIAG-3 "phase (setup/main/cleanup) in the JSON and the CSV" $res

# ---- DIAG-4: under a PM, which operator sequence was run ----
# `sequence` is the PM's name, so the test has to travel as a field of its
# own: without it, the archived result does not record what was run.
$A --process-model $R/pm-minimal.yaml \
   ejemplos/limites.yaml --json "$TMP/d4.json" >/dev/null 2>&1
grep -q '"user_sequence": "ejemplos/limites.yaml"' "$TMP/d4.json"
check DIAG-4 "the operator's sequence is a JSON field" $?

# ---- DIAG-5a: a wrapped sidecar must point at the wrapper ----
# The generic error blamed the step's name, which is fine; that is where the
# phantom bug "the sidecar does not work with a process model" came from.
cat >"$TMP/envoltorio.limits.yaml" <<'YAML'
limits:
  demo/measure_voltage:
    comparison: GE
    low: 4.0
YAML
$A ejemplos/limites.yaml --limits "$TMP/envoltorio.limits.yaml" 2>&1 |
  grep -qiE 'mapa plano|envoltorio'
check DIAG-5a "a wrapped sidecar points at the wrapper, not the step" $?

# ---- DIAG-5b: an unknown field must be located and the right one suggested ----
cat >"$TMP/steps.yaml" <<'YAML'
name: regresion_steps
subsequences:
  interna:
    steps:
      - name: p
        type: pass_fail
        module: p
main:
  - name: p
    type: pass_fail
    module: p
YAML
$A "$TMP/steps.yaml" --validate 2>&1 |
  grep -qE "subsequences.interna.*querías 'main'"
check DIAG-5b "unknown field: location + suggestion" $?

# ---- DIAG-5c: unknown flag message ----
# The original pattern required the adjective AFTER the flag's name and never
# matched the real message ("flag desconocido: '--x'"), so this case stayed red
# long after it was fixed. It now accepts both orders.
$A "$R/bug2-csv-nombre.yaml" --inventado 2>&1 |
  grep -qiE "flag (desconocido|no reconocido).*--inventado|flag .*--inventado.* (desconocido|no reconocido)"
check DIAG-5c "an unknown flag is reported as unknown" $?

# ---- DIAG-5e: -h and -V exist (the beta did not use them once) ----
$A -h 2>&1 | grep -qE '^uso: anvil' && $A -V 2>&1 | grep -qE '^anvil [0-9]'
check DIAG-5e "-h and -V answer like --help and --version" $?

# ---- DIAG-5d: a .wasm that is a core module, not a component ----
# The 8 header bytes are a valid, empty core module: enough for the executor to
# refuse it, and the message must say WHY (it used to say only "failed to parse
# WebAssembly module", which got the toolchain blamed).
#
# Since ADR-0027 the YAML's `path` is the executor's binary, so the bad module
# goes INSIDE the department: a fake one is assembled with a copy of the
# executor and the core.wasm beside it.
PUENTE=""
for cand in packaging/anvil-host/target/release/anvil-exec-wasm \
            packaging/anvil-host/target/debug/anvil-exec-wasm \
            executors/wasm/target/release/anvil-exec-wasm \
            executors/wasm/target/debug/anvil-exec-wasm; do
  [ -x "$cand" ] && { PUENTE="$cand"; break; }
done
if [ -z "$PUENTE" ]; then
  check DIAG-5d "a core-module .wasm is diagnosed as such (no executor: skipped)" 1
else
  mkdir -p "$TMP/depto"
  cp "$PUENTE" "$TMP/depto/anvil-exec-wasm"
  printf '\x00asm\x01\x00\x00\x00' >"$TMP/depto/core.wasm"
  cat >"$TMP/coremod.yaml" <<'YAML'
name: regresion_modulo_core
executors:
  - name: dmm
    type: wasm
    path: ./depto/anvil-exec-wasm
main:
  - name: core/medir
    type: pass_fail
    module: core/medir
    executor: dmm
YAML
  $A "$TMP/coremod.yaml" 2>&1 | grep -qiE 'módulo core|modulo core|core module'
  check DIAG-5d "a core-module .wasm is diagnosed as such" $?
fi

# ---- DIAG-5g: pointing a wasm executor's `path` at a `.wasm` ----
# The number one stumble coming from before ADR-0027. It is a file, so it passes
# any existence check, and `exec` would fail with "Exec format error" — which
# sends you to look at the toolchain instead of the YAML line.
printf '\x00asm\x0d\x00\x01\x00' >"$TMP/suelto.wasm"
cat >"$TMP/pathwasm.yaml" <<'YAML'
name: regresion_path_es_wasm
executors:
  - name: dmm
    type: wasm
    path: ./suelto.wasm
main:
  - name: x/medir
    type: pass_fail
    module: x/medir
    executor: dmm
YAML
$A "$TMP/pathwasm.yaml" 2>&1 | grep -qiE 'binario del ejecutor|executor.s binary'
check DIAG-5g "a path to a .wasm says the executor's binary is expected" $?

# ---- LEC-1: `result.*` outside `assign` must be a load error ----
# The product lesson (§5): this YAML loaded, the precondition was a constant
# `false`, the step was skipped and the sequence came out GREEN. The campaign
# spread the pattern to 19 sequences and 51 preconditions.
cat >"$TMP/lec1.yaml" <<'YAML'
name: regresion_result_outside_assign
executors:
  - { name: demo, type: wasm, path: ../../../../ejemplos/departamento/dist/anvil-exec-wasm }
locals:
  v_real: 5.0
main:
  - name: demo/measure_voltage
    type: pass_fail
    module: demo/measure_voltage
    executor: demo
    precondition: 'locals.v_real > 4.9 && result.measured_value != nothing'
YAML
$A "$TMP/lec1.yaml" --validate 2>&1 |
  grep -qE "measure_voltage.*result.measured_value|result.measured_value.*precondicion"
check LEC-1 "result.* in a precondition is a load error" $?

# ---- LEC-2: a green that skipped steps has to say so ----
# `skipped` is neutral in the aggregate and must stay so, but 9 sequences of the
# campaign came out green skipping ≥30% of their steps without it showing.
cat >"$TMP/lec2.yaml" <<'YAML'
name: regresion_visible_skips
executors:
  - { name: demo, type: wasm, path: ../../../../ejemplos/departamento/dist/anvil-exec-wasm }
locals:
  activo: false
main:
  - name: preparar
    type: statement
    statement: 'locals.activo = false'
  - name: demo/measure_voltage
    type: action
    module: demo/measure_voltage
    executor: demo
    precondition: 'locals.activo'
  - name: demo/check_led
    type: action
    module: demo/check_led
    executor: demo
    disable: true
YAML
# Actions, not pass_fail steps: a sequence whose declared verdicts were all
# skipped is inconclusive (ADR-0019 Rule 1, #31), which is another case. This
# one is about skips being neutral.
$A "$TMP/lec2.yaml" --json "$TMP/lec2.json" >"$TMP/lec2.out" 2>/dev/null
res=0
grep -qE '\(2 de 3 pasos saltados\)' "$TMP/lec2.out" || res=1
grep -q '"skipped_steps": 2' "$TMP/lec2.json" || res=1
grep -q '"total_steps": 3' "$TMP/lec2.json" || res=1
# And the aggregate stays green: neutrality does not change (RF-33/34).
grep -q ': pass ===' "$TMP/lec2.out" || res=1
check LEC-2 "a green with skipped steps declares it (console and JSON)" $res

# ---- NOTA-1: two simultaneous `anvil` must not clash on a port ----
# The executor once built into anvil bound a fixed 9100: the second process
# died with `address in use`, which prevented parallelising a campaign by
# launching N processes. That executor is gone (ADR-0041); the case stays
# because each run still spawns its declared executors on ports of its own.
$A ejemplos/basica.yaml >"$TMP/n1a.out" 2>&1 &
p1=$!
$A ejemplos/basica.yaml >"$TMP/n1b.out" 2>&1 &
p2=$!
wait $p1; wait $p2
res=0
for f in "$TMP/n1a.out" "$TMP/n1b.out"; do
  grep -qi 'address in use\|refused' "$f" && res=1
  grep -q '=== basica:' "$f" || res=1
done
check NOTA-1 "two simultaneous anvil run without a port clash" $res

# ---- EXIT-1: the exit code must reflect the aggregate verdict (#16) ----
# `main` discarded `ejecuta_programa`'s `Ok` and only looked at the `Err` (which
# is "the communication broke", not "the verdict is negative"), so a red
# sequence exited 0 and `anvil sequence.yaml && deploy` deployed with the DUT
# failed. Contract: 0 only if the aggregate is `pass`, 1 for everything else.
# The `error` fixture is the host tests' one: no example produces a
# deterministic execution error without a network.
F=packaging/anvil-host/tests/fixtures
res=0
$A "$F/paso.yaml"          --quiet >/dev/null 2>&1; [ $? -eq 0 ] || res=1
$A ejemplos/veredicto.yaml --quiet >/dev/null 2>&1; [ $? -eq 1 ] || res=1
$A "$F/error_runtime.yaml" --quiet >/dev/null 2>&1; [ $? -eq 1 ] || res=1
$A no-existe-de-verdad.yaml        >/dev/null 2>&1; [ $? -eq 1 ] || res=1
check EXIT-1 "exit 0 only on a pass verdict; fail/error/load exit 1" $res

echo "===================================================================="
echo "  OK: $ok    FALLA: $falla"
[ "$falla" -eq 0 ]
