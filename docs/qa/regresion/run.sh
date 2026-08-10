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
# Requiere el binario host construido:
#   make release      (o `make build`, pero arranca mucho más lento)

cd "$(dirname "$0")/../../.." || exit 1

A=""
for cand in packaging/anvil-host/target/release/anvil \
            packaging/anvil-host/target/debug/anvil; do
  [ -x "$cand" ] && { A="$cand"; break; }
done
if [ -z "$A" ]; then
  echo "no encuentro el binario host. Constrúyelo con:" >&2
  echo "  make release" >&2
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
$A --process-model process_models/sequential.yaml \
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
# Antes este caso reusaba el comando de DEF-1, que daba 0 afectados **por el
# defecto**. Arreglado DEF-1, hace falta un sidecar huérfano de verdad.
cat >"$TMP/huerfano.limits.yaml" <<'YAML'
paso_que_no_existe:
  tipo: comparacion
  op: ge
  esperado: 4.0
YAML
$A ejemplos/limites.yaml --limits "$TMP/huerfano.limits.yaml" 2>&1 |
  grep -qiE 'aviso.*sidecar|sidecar.*no afect|ningún paso'
check DIAG-1 "aviso cuando el sidecar afecta a 0 pasos" $?

# ---- DIAG-3: el reporte debe decir en qué fase corrió cada paso ----
# Sin esto, al post-procesar no se distingue un fallo de Setup (el DUT no se
# pudo ni conectar) de uno de Main o de Cleanup.
$A ejemplos/basica.yaml --json "$TMP/d3.json" --csv "$TMP/d3.csv" >/dev/null 2>&1
res=0
for f in setup main cleanup; do
  grep -q "\"fase\": \"$f\"" "$TMP/d3.json" || res=1
done
# El CSV va en CRLF (RFC-4180): hay que quitar el \r antes de anclar a fin.
head -1 "$TMP/d3.csv" 2>/dev/null | tr -d '\r' | grep -q ',fase$' || res=1
check DIAG-3 "fase (setup/main/cleanup) en el JSON y en el CSV" $res

# ---- DIAG-4: bajo un PM, qué secuencia de operador se corrió ----
# `secuencia` es el nombre del PM, así que el test debe viajar como campo
# propio: sin él, el resultado archivado no registra qué se corrió.
$A --process-model process_models/sequential.yaml \
   ejemplos/limites.yaml --json "$TMP/d4.json" >/dev/null 2>&1
grep -q '"secuencia_usuario": "ejemplos/limites.yaml"' "$TMP/d4.json"
check DIAG-4 "la secuencia del operador es un campo del JSON" $?

# ---- DIAG-5a: un sidecar envuelto debe señalar el envoltorio ----
# El error genérico acusaba al nombre del paso, que está bien; de ahí salió el
# bug fantasma «el sidecar no funciona con process model».
cat >"$TMP/envoltorio.limits.yaml" <<'YAML'
limites:
  medir_voltaje:
    tipo: comparacion
    op: ge
    esperado: 4.0
YAML
$A ejemplos/limites.yaml --limits "$TMP/envoltorio.limits.yaml" 2>&1 |
  grep -qiE 'mapa plano|envoltorio'
check DIAG-5a "un sidecar envuelto señala el envoltorio, no el paso" $?

# ---- DIAG-5b: un campo desconocido debe ubicarse y sugerir el correcto ----
cat >"$TMP/steps.yaml" <<'YAML'
nombre: regresion_steps
subsecuencias:
  interna:
    steps:
      - nombre: p
main:
  - nombre: p
YAML
$A "$TMP/steps.yaml" --validate 2>&1 |
  grep -qE "subsecuencias.interna.*querías 'main'"
check DIAG-5b "campo desconocido: ubicación + sugerencia" $?

# ---- DIAG-5c: mensaje de flag desconocido ----
# El patrón original exigía el adjetivo DESPUÉS del nombre del flag y nunca
# casó con el mensaje real ("flag desconocido: '--x'"), así que este caso salía
# rojo mucho después de estar arreglado. Ahora acepta ambos órdenes.
$A "$R/bug2-csv-nombre.yaml" --inventado 2>&1 |
  grep -qiE "flag (desconocido|no reconocido).*--inventado|flag .*--inventado.* (desconocido|no reconocido)"
check DIAG-5c "flag desconocido se reporta como desconocido" $?

# ---- DIAG-5e: -h y -V existen (la beta no los usó ni una vez) ----
$A -h 2>&1 | grep -qE '^uso: anvil' && $A -V 2>&1 | grep -qE '^anvil [0-9]'
check DIAG-5e "-h y -V responden como --help y --version" $?

# ---- DIAG-5f: nada de ejecutor si el motor no va a correr un paso ----
# Anunciar 'escuchando en 9100' por delante de la ayuda o del error ensucia la
# salida, y con el puerto fijo del MVP bloquea a otro anvil que sí fuera a
# correr (dos `--validate` en paralelo chocaban).
! $A -h 2>&1 | grep -qi 'escuchando' &&
  ! $A ejemplos/limites.yaml --validate 2>&1 | grep -qi 'escuchando'
check DIAG-5f "sin ejecutor embebido para -h/--validate" $?

# ---- DIAG-5d: un .wasm que es módulo core, no componente ----
# Los 8 bytes de cabecera son un módulo core válido y vacío: basta para que el
# puente lo rechace, y el mensaje debe decir POR QUÉ (antes decía sólo
# "failed to parse WebAssembly module", que hizo culpar al toolchain).
printf '\x00asm\x01\x00\x00\x00' >"$TMP/core.wasm"
cat >"$TMP/coremod.yaml" <<'YAML'
nombre: regresion_modulo_core
ejecutores:
  - nombre: dmm
    tipo: wasm
    path: ./core.wasm
main:
  - nombre: medir
    ejecutor: dmm
YAML
$A "$TMP/coremod.yaml" 2>&1 | grep -qiE 'módulo core|modulo core'
check DIAG-5d "un .wasm módulo core se diagnostica como tal" $?

# ---- LEC-1: `resultado.*` fuera de `asigna` debe ser error de carga ----
# La lección de producto (§5): este YAML cargaba, la precondición era un `false`
# constante, el paso se saltaba y la secuencia salía VERDE. La campaña propagó
# el patrón a 19 secuencias y 51 precondiciones.
cat >"$TMP/lec1.yaml" <<'YAML'
nombre: regresion_resultado_fuera_de_asigna
locals:
  v_real: 5.0
main:
  - nombre: medir_voltaje
    precondicion: 'locals.v_real > 4.9 && resultado.valor_medido != nothing'
YAML
$A "$TMP/lec1.yaml" --validate 2>&1 |
  grep -qE "medir_voltaje.*resultado.valor_medido|resultado.valor_medido.*precondicion"
check LEC-1 "resultado.* en una precondición es error de carga" $?

# ---- LEC-2: un verde que se saltó pasos tiene que decirlo ----
# `saltado` es neutral en el agregado y debe seguir siéndolo, pero 9 secuencias
# de la campaña daban verde saltándose ≥30% de sus pasos sin que se notara.
cat >"$TMP/lec2.yaml" <<'YAML'
nombre: regresion_saltos_visibles
locals:
  activo: false
main:
  - nombre: preparar
    tipo: statement
    statement: 'locals.activo = false'
  - nombre: medir_voltaje
    precondicion: 'locals.activo'
  - nombre: verificar_led
    disable: true
YAML
$A "$TMP/lec2.yaml" --json "$TMP/lec2.json" >"$TMP/lec2.out" 2>/dev/null
res=0
grep -qE '\(2 de 3 pasos saltados\)' "$TMP/lec2.out" || res=1
grep -q '"pasos_saltados": 2' "$TMP/lec2.json" || res=1
grep -q '"pasos_totales": 3' "$TMP/lec2.json" || res=1
# Y el agregado sigue siendo verde: la neutralidad no cambia (RF-33/34).
grep -q ': paso ===' "$TMP/lec2.out" || res=1
check LEC-2 "un verde con pasos saltados lo declara (consola y JSON)" $res

echo "===================================================================="
echo "  OK: $ok    FALLA: $falla"
[ "$falla" -eq 0 ]
