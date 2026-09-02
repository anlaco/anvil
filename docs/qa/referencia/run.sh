#!/bin/bash
# ADR-0022 end to end: an object that stays in the executor, and a handle to it
# that the sequence carries from step to step.
#
# Unlike the unit tests, this one needs the wire: a real Python executor, a real
# instrument session over TCP, and a real restart of the process holding the
# objects. It is the half of the acceptance criteria that cannot be decided by
# reading a file.
#
# Usage (from the repo root):
#   ./docs/qa/referencia/run.sh
#
# Needs the host binary (`make release`) and `grpcio`. If `grpcio` is missing it
# says so and exits 3: a check that quietly does not run is worse than one that
# is not there. To install it without touching the environment:
#   python3 -m pip install --target <dir> grpcio
#   PYTHONPATH=<dir> ./docs/qa/referencia/run.sh

cd "$(dirname "$0")/../../.." || exit 1

A=""
for cand in packaging/anvil-host/target/release/anvil \
            packaging/anvil-host/target/debug/anvil; do
  [ -x "$cand" ] && { A="$cand"; break; }
done
if [ -z "$A" ]; then
  echo "no encuentro el binario host. Constrúyelo con: make release" >&2
  exit 2
fi

if ! python3 -c 'import grpc' 2>/dev/null; then
  echo "sin 'grpcio': este check necesita levantar el ejecutor Python de verdad." >&2
  echo "  python3 -m pip install --target <dir> grpcio" >&2
  echo "  PYTHONPATH=<dir> $0" >&2
  exit 3
fi

R=docs/qa/referencia
# El binario solo preabre el CWD: la salida de --json/--csv cae dentro del árbol.
TMP=$R/.tmp; mkdir -p "$TMP"
export PYTHONPATH="$PWD/executors/python:$PYTHONPATH"

PIDS=()
limpia() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
  rm -rf "$TMP"
}
trap limpia EXIT

arranca_banco() {  # arranca el ejecutor con los pasos de banco en 9101
  python3 executors/python/server.py --port 9101 \
    --steps executors/python/steps --steps "$R/steps_qa" >"$TMP/banco.log" 2>&1 &
  BANCO=$!
  PIDS+=("$BANCO")
  for _ in $(seq 1 50); do
    grep -q 'listening' "$TMP/banco.log" && return 0
    sleep 0.1
  done
  echo "el ejecutor de banco no arrancó:" >&2; cat "$TMP/banco.log" >&2; return 1
}

ok=0; falla=0
check() {  # check <id> <descripcion> <0=ok|1=falla>
  if [ "$3" -eq 0 ]; then
    printf '  \033[32mOK   \033[0m %-8s %s\n' "$1" "$2"; ok=$((ok+1))
  else
    printf '  \033[31mFALLA\033[0m %-8s %s\n' "$1" "$2"; falla=$((falla+1))
  fi
}

echo "== ADR-0022: referencia a objeto, de punta a punta ================="
echo "binario: $A"

# Simulador TCP: el instrumento contra el que se abre la sesión.
python3 executors/python/simulador_tcp.py >"$TMP/sim.log" 2>&1 &
PIDS+=($!)
# Reloj: un segundo ejecutor, sólo para poder parar la corrida entre dos pasos.
python3 executors/python/server.py --port 9102 --steps "$R/steps_pacer" \
  >"$TMP/reloj.log" 2>&1 &
PIDS+=($!)
sleep 1
arranca_banco || exit 1

# ---- REF-1: un banco abierto, usado por varios pasos y cerrado ------------
# El criterio central: el objeto se queda en el ejecutor, la secuencia lleva la
# referencia, y el identificador queda escrito en el informe (Regla 3 de
# ADR-0019: se puede reconstruir contra qué banco se midió).
$A ejemplos/referencia.yaml --json "$TMP/r1.json" --csv "$TMP/r1.csv" \
  >"$TMP/r1.out" 2>"$TMP/r1.err"
salida=$?
res=0
[ $salida -eq 0 ] || res=1
# El mismo casillero en los cuatro pasos: mutar el banco no cambia su identidad.
payloads=$(grep -o '"payload": "[^"]*"' "$TMP/r1.json" | sort -u | wc -l)
[ "$payloads" -eq 1 ] || res=1
# Y hay identificador en el informe, no una cadena vacía.
grep -q '"type": "reference"' "$TMP/r1.json" || res=1
grep -q 'bench=ref:python/' "$TMP/r1.csv" || res=1
# La medida salió, y por la sesión abierta en el setup.
grep -q '"measured_value": 4.8' "$TMP/r1.json" || res=1
check REF-1 "un banco abierto, usado en varios pasos y cerrado (payloads distintos: $payloads, esperado 1)" $res

# ---- REF-2: el ejecutor se reinicia a mitad de corrida --------------------
# Se detecta ANTES de invocar el paso, el paso no mide, y —lo que decide entre
# un `error` y un aborto— el `cleanup` llega a correr.
cat >"$TMP/reinicio.yaml" <<'YAML'
name: regresion_reinicio
executors:
  - { name: banco, type: grpc, host: 127.0.0.1, port: 9101 }
  - { name: reloj, type: grpc, host: 127.0.0.1, port: 9102 }
locals:
  bench: { type: reference, executor: banco }
setup:
  - name: instrument/open_bench
    executor: banco
    assign:
      bench: result.outputs.bench
main:
  - name: pacer/wait
    executor: reloj
    inputs: { seconds: 4 }
  - name: instrument/measure_bench
    executor: banco
    inputs:
      bench: '${locals.bench}'
cleanup:
  - name: instrument/close_bench
    executor: banco
    inputs:
      bench: '${locals.bench}'
YAML
$A "$TMP/reinicio.yaml" --json "$TMP/r2.json" >"$TMP/r2.out" 2>"$TMP/r2.err" &
CORRIDA=$!
# Durante el `wait`: matar el ejecutor de banco y levantar otro. Es un proceso
# nuevo, así que acuña una vida nueva y su mapa de objetos está vacío.
sleep 2
kill "$BANCO" 2>/dev/null; wait "$BANCO" 2>/dev/null
arranca_banco || exit 1
wait "$CORRIDA"; salida=$?
res=0
[ $salida -eq 1 ] || res=1
python3 - "$TMP/r2.json" <<'PY' || res=1
import json, sys
pasos = {p["name"]: p for p in json.load(open(sys.argv[1]))["steps"]}
medir = pasos.get("instrument/measure_bench")
cierre = pasos.get("instrument/close_bench")
assert medir, "el paso de medida no está en el informe"
assert medir["status"] == "error", f"medir salió {medir['status']}, no error"
assert medir["measured_value"] is None, "no debería haber medido nada"
assert cierre, "el cleanup no corrió: la corrida se paró en seco"
assert cierre["phase"] == "cleanup"
PY
check REF-2 "un ejecutor reiniciado se detecta antes de medir y el cleanup corre" $res

# ---- REF-3: una carga opaca con ';' y '=' no corrompe el CSV --------------
cat >"$TMP/opaca.yaml" <<'YAML'
name: regresion_carga_opaca
executors:
  - { name: banco, type: grpc, host: 127.0.0.1, port: 9101 }
locals:
  handle: { type: reference, executor: banco }
main:
  - name: nasty/mint_nasty_reference
    executor: banco
    assign:
      handle: result.outputs.handle
YAML
$A "$TMP/opaca.yaml" --csv "$TMP/r3.csv" >/dev/null 2>&1
res=0
celda=$(sed -n '2p' "$TMP/r3.csv" | tr -d '\r' | awk -F, '{print $NF}')
# Un solo par en la celda: ni el ';' ni el '=' del payload separan nada.
[ "$(printf '%s' "$celda" | tr -cd ';' | wc -c)" -eq 0 ] || res=1
[ "$(printf '%s' "$celda" | tr -cd '=' | wc -c)" -eq 1 ] || res=1
printf '%s' "$celda" | grep -q '^handle=ref:' || res=1
check REF-3 "una carga opaca con ';' y '=' no parte la celda del CSV (celda: $celda)" $res

echo "===================================================================="
echo "  OK: $ok    FALLA: $falla"
[ "$falla" -eq 0 ]
