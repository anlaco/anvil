#!/bin/bash
# Regresión de los defectos de la beta de agosto 2026.
# Ver docs/qa/informe-beta-2026-08.md
#
# Cada caso AFIRMA EL COMPORTAMIENTO CORRECTO, así que mientras el defecto siga
# presente sale FALLA. Cuando se arregle, sale OK.
#
# Uso (desde la raíz del repo):
#   ./docs/qa/regresion/run.sh
#
# Requiere el binario host construido (ver docs/guia-inicio-rapido.md):
#   cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos
#   cargo build --manifest-path packaging/anvil-host/Cargo.toml

cd "$(dirname "$0")/../../.." || exit 1

A=""
for cand in packaging/anvil-host/target/release/anvil \
            packaging/anvil-host/target/debug/anvil; do
  [ -x "$cand" ] && { A="$cand"; break; }
done
if [ -z "$A" ]; then
  echo "no encuentro el binario host. Constrúyelo con:" >&2
  echo "  cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos" >&2
  echo "  cargo build --manifest-path packaging/anvil-host/Cargo.toml" >&2
  exit 2
fi
echo "binario: $A"

R=docs/qa/regresion
ok=0; falla=0
# El binario solo preabre el CWD: la salida de --csv/--json debe caer dentro
# del árbol, no en /tmp.
TMP=$R/.tmp; mkdir -p "$TMP"; trap 'rm -rf "$TMP"' EXIT

check() {  # check <id> <descripcion> <0=ok|1=falla>
  if [ "$3" -eq 0 ]; then
    printf '  \033[32mOK   \033[0m %-8s %s\n' "$1" "$2"; ok=$((ok+1))
  else
    printf '  \033[31mFALLA\033[0m %-8s %s\n' "$1" "$2"; falla=$((falla+1))
  fi
}

echo "== Regresión beta 2026-08 =========================================="

# ---- DEF-1: --limits debe llegar a la secuencia del operador bajo un PM ----
$A --process-model ejemplos/process_model_sequential.yaml \
   ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml \
   >"$TMP/d1.out" 2>"$TMP/d1.err"
afectados=$(grep -oE 'aplicado \([0-9]+ paso' "$TMP/d1.err" | grep -oE '[0-9]+' | head -1)
grep -q ': paso ===' "$TMP/d1.out"; agregado=$?
[ "${afectados:-0}" -ge 1 ] && [ "$agregado" -eq 0 ]
check DEF-1 "sidecar aplicado bajo --process-model (afectados=${afectados:-0}, esperado>=1)" $?

# ---- DEF-2: la columna nombre_secuencia del CSV debe traer el nombre ----
$A "$R/bug2-csv-nombre.yaml" --csv "$TMP/d2.csv" >/dev/null 2>&1
col1=$(sed -n '2p' "$TMP/d2.csv" 2>/dev/null | cut -d, -f1)
[ "$col1" = "regresion_csv_nombre" ]
check DEF-2 "CSV columna 1 = nombre de secuencia (obtenido: '${col1}')" $?

# ---- DEF-3a: asigna no debe ensombrecer un parameter en silencio ----
$A "$R/bug3-sub-asigna-parameter.yaml" >"$TMP/d3a.out" 2>"$TMP/d3a.err"
if grep -qiE 'inválida|invalida' "$TMP/d3a.err"; then
  res=0   # el cargador lo rechaza: arreglo aceptable
else
  grep -qE '\[saltado\] verificar_led' "$TMP/d3a.out" && res=1 || res=0
fi
check DEF-3a "asigna no ensombrece un parameter declarado" $res

# ---- DEF-3b: el retorno by-reference debe traer el valor medido al padre ----
$A "$R/bug3-padre-asigna-parameter.yaml" >"$TMP/d3b.out" 2>&1
grep -qE '\[saltado\] verificar_led' "$TMP/d3b.out" && res=1 || res=0
check DEF-3b "retorno by-reference trae el valor medido al padre" $res

# ---- DIAG-1: avisar cuando el sidecar no afecta a ningún paso ----
$A --process-model ejemplos/process_model_sequential.yaml \
   ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml 2>&1 |
  grep -qiE 'aviso.*sidecar|sidecar.*no afect|ningún paso'
check DIAG-1 "aviso cuando el sidecar afecta a 0 pasos" $?

# ---- DIAG-5: mensaje de flag desconocido ----
$A "$R/bug2-csv-nombre.yaml" --inventado 2>&1 |
  grep -qiE "flag .*--inventado.* (desconocido|no reconocido)"
check DIAG-5 "flag desconocido se reporta como desconocido" $?

echo "===================================================================="
echo "  OK: $ok    FALLA: $falla"
[ "$falla" -eq 0 ]
