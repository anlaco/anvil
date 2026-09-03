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
grep -q ': pass ===' "$TMP/d1.out"; agregado=$?
[ "${afectados:-0}" -ge 1 ] && [ "$agregado" -eq 0 ]
check DEF-1 "sidecar aplicado bajo --process-model (afectados=${afectados:-0}, esperado>=1)" $?

# ---- DEF-2: la columna nombre_secuencia del CSV debe traer el nombre ----
$A "$R/bug2-csv-nombre.yaml" --csv "$TMP/d2.csv" >/dev/null 2>&1
col1=$(sed -n '2p' "$TMP/d2.csv" 2>/dev/null | cut -d, -f1)
[ "$col1" = "regresion_csv_nombre" ]
check DEF-2 "CSV columna 1 = nombre de secuencia (obtenido: '${col1}')" $?

# ---- DEF-3a / DEF-3b: asigna no debe ensombrecer un parameter en silencio ----
# El defecto se cerró por la vía dura —el cargador rechaza la secuencia— así que
# los dos casos aceptan el rechazo como arreglo. Lo que **no** aceptan es la
# nada: sin la guardia de abajo, un binario que no imprimiera una sola línea
# salía verde, porque el `grep` de la marca de fallo no casaba con un fichero
# vacío. DEF-3b pasaba exactamente así.
asigna_no_ensombrece() {  # <stdout> <stderr>
  if grep -qiE 'inválida|invalida' "$2"; then
    return 0            # el cargador lo rechaza: arreglo aceptable
  fi
  if ! grep -q '===' "$1"; then
    return 1            # ni rechazo ni corrida: aquí no se ha comprobado nada
  fi
  grep -qE '\[skipped\] verificar_led' "$1" && return 1 || return 0
}

$A "$R/bug3-sub-asigna-parameter.yaml" >"$TMP/d3a.out" 2>"$TMP/d3a.err"
asigna_no_ensombrece "$TMP/d3a.out" "$TMP/d3a.err"
check DEF-3a "asigna no ensombrece un parameter declarado" $?

# El mismo shadow visto desde el padre: el retorno by-reference no puede traerse
# el valor inicial en vez del medido.
$A "$R/bug3-padre-asigna-parameter.yaml" >"$TMP/d3b.out" 2>"$TMP/d3b.err"
asigna_no_ensombrece "$TMP/d3b.out" "$TMP/d3b.err"
check DEF-3b "retorno by-reference no trae el valor sin medir" $?

# ---- DIAG-1: avisar cuando el sidecar no afecta a ningún paso ----
# Antes este caso reusaba el comando de DEF-1, que daba 0 afectados **por el
# defecto**. Arreglado DEF-1, hace falta un sidecar huérfano de verdad.
cat >"$TMP/huerfano.limits.yaml" <<'YAML'
paso_que_no_existe:
  type: comparison
  op: ge
  expected: 4.0
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
  grep -q "\"phase\": \"$f\"" "$TMP/d3.json" || res=1
done
# El CSV va en CRLF (RFC-4180): hay que quitar el \r antes de mirar.
# La columna no se ancla al final: lo que este caso afirma es que la fase esté,
# no en qué posición — `inputs` y `outputs` se añadieron después (ADR-0020) y
# la dejaron en medio.
head -1 "$TMP/d3.csv" 2>/dev/null | tr -d '\r' | grep -qE '(^|,)phase(,|$)' || res=1
check DIAG-3 "fase (setup/main/cleanup) en el JSON y en el CSV" $res

# ---- DIAG-4: bajo un PM, qué secuencia de operador se corrió ----
# `secuencia` es el nombre del PM, así que el test debe viajar como campo
# propio: sin él, el resultado archivado no registra qué se corrió.
$A --process-model process_models/sequential.yaml \
   ejemplos/limites.yaml --json "$TMP/d4.json" >/dev/null 2>&1
grep -q '"user_sequence": "ejemplos/limites.yaml"' "$TMP/d4.json"
check DIAG-4 "la secuencia del operador es un campo del JSON" $?

# ---- DIAG-5a: un sidecar envuelto debe señalar el envoltorio ----
# El error genérico acusaba al nombre del paso, que está bien; de ahí salió el
# bug fantasma «el sidecar no funciona con process model».
cat >"$TMP/envoltorio.limits.yaml" <<'YAML'
limits:
  medir_voltaje:
    type: comparison
    op: ge
    expected: 4.0
YAML
$A ejemplos/limites.yaml --limits "$TMP/envoltorio.limits.yaml" 2>&1 |
  grep -qiE 'mapa plano|envoltorio'
check DIAG-5a "un sidecar envuelto señala el envoltorio, no el paso" $?

# ---- DIAG-5b: un campo desconocido debe ubicarse y sugerir el correcto ----
cat >"$TMP/steps.yaml" <<'YAML'
name: regresion_steps
subsequences:
  interna:
    steps:
      - name: p
main:
  - name: p
YAML
$A "$TMP/steps.yaml" --validate 2>&1 |
  grep -qE "subsequences.interna.*querías 'main'"
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
# ejecutor lo rechace, y el mensaje debe decir POR QUÉ (antes decía sólo
# "failed to parse WebAssembly module", que hizo culpar al toolchain).
#
# Desde ADR-0027 el `path` del YAML es el binario del ejecutor, así que el
# módulo malo se pone DENTRO del departamento: se monta uno de mentira con una
# copia del ejecutor y el core.wasm al lado.
PUENTE=""
for cand in packaging/anvil-host/target/release/anvil-exec-wasm \
            packaging/anvil-host/target/debug/anvil-exec-wasm \
            executors/wasm/target/release/anvil-exec-wasm \
            executors/wasm/target/debug/anvil-exec-wasm; do
  [ -x "$cand" ] && { PUENTE="$cand"; break; }
done
if [ -z "$PUENTE" ]; then
  check DIAG-5d "un .wasm módulo core se diagnostica como tal (sin ejecutor: omitido)" 1
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
    executor: dmm
YAML
  $A "$TMP/coremod.yaml" 2>&1 | grep -qiE 'módulo core|modulo core|core module'
  check DIAG-5d "un .wasm módulo core se diagnostica como tal" $?
fi

# ---- DIAG-5g: apuntar el `path` de un ejecutor wasm a un `.wasm` ----
# El tropiezo nº1 viniendo de antes de ADR-0027. Es un fichero, así que pasa
# cualquier comprobación de existencia, y `exec` fallaría con «Exec format
# error» — que manda a mirar el toolchain en vez de la línea del YAML.
printf '\x00asm\x0d\x00\x01\x00' >"$TMP/suelto.wasm"
cat >"$TMP/pathwasm.yaml" <<'YAML'
name: regresion_path_es_wasm
executors:
  - name: dmm
    type: wasm
    path: ./suelto.wasm
main:
  - name: x/medir
    executor: dmm
YAML
$A "$TMP/pathwasm.yaml" 2>&1 | grep -qiE 'binario del ejecutor|executor.s binary'
check DIAG-5g "path a un .wasm dice que se espera el binario del ejecutor" $?

# ---- LEC-1: `resultado.*` fuera de `asigna` debe ser error de carga ----
# La lección de producto (§5): este YAML cargaba, la precondición era un `false`
# constante, el paso se saltaba y la secuencia salía VERDE. La campaña propagó
# el patrón a 19 secuencias y 51 precondiciones.
cat >"$TMP/lec1.yaml" <<'YAML'
name: regresion_result_outside_assign
locals:
  v_real: 5.0
main:
  - name: medir_voltaje
    precondition: 'locals.v_real > 4.9 && result.measured_value != nothing'
YAML
$A "$TMP/lec1.yaml" --validate 2>&1 |
  grep -qE "medir_voltaje.*result.measured_value|result.measured_value.*precondicion"
check LEC-1 "resultado.* en una precondición es error de carga" $?

# ---- LEC-2: un verde que se saltó pasos tiene que decirlo ----
# `saltado` es neutral en el agregado y debe seguir siéndolo, pero 9 secuencias
# de la campaña daban verde saltándose ≥30% de sus pasos sin que se notara.
cat >"$TMP/lec2.yaml" <<'YAML'
name: regresion_visible_skips
locals:
  activo: false
main:
  - name: preparar
    type: statement
    statement: 'locals.activo = false'
  - name: medir_voltaje
    precondition: 'locals.activo'
  - name: verificar_led
    disable: true
YAML
$A "$TMP/lec2.yaml" --json "$TMP/lec2.json" >"$TMP/lec2.out" 2>/dev/null
res=0
grep -qE '\(2 de 3 pasos saltados\)' "$TMP/lec2.out" || res=1
grep -q '"skipped_steps": 2' "$TMP/lec2.json" || res=1
grep -q '"total_steps": 3' "$TMP/lec2.json" || res=1
# Y el agregado sigue siendo verde: la neutralidad no cambia (RF-33/34).
grep -q ': pass ===' "$TMP/lec2.out" || res=1
check LEC-2 "un verde con pasos saltados lo declara (consola y JSON)" $res

# ---- NOTA-1: dos `anvil` simultáneos no deben chocar de puerto ----
# El ejecutor embebido bindeaba 9100 fijo: el segundo proceso moría con
# `address in use`, lo que impedía paralelizar una campaña lanzando N procesos.
# El `--port` que la guía recomendaba como remedio sólo movía la punta del
# motor, así que daba `connection refused`.
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
check NOTA-1 "dos anvil simultáneos corren sin chocar de puerto" $res

# ---- NOTA-1b: --port explícito fija ejecutor y motor, no sólo el motor ----
$A ejemplos/basica.yaml --port 9300 2>&1 | grep -qE 'escuchando en 9300'
check NOTA-1b "--port fija también el puerto del ejecutor embebido" $?

# ---- EXIT-1: el exit code debe reflejar el veredicto agregado (#16) ----
# `main` descartaba el `Ok` de `ejecuta_programa` y sólo miraba el `Err` (que
# es «se rompió la comunicación», no «el veredicto es negativo»), así que una
# secuencia en rojo salía 0 y `anvil secuencia.yaml && desplegar` desplegaba
# con el DUT suspendido. Contrato: 0 sólo si el agregado es `paso`, 1 en todo
# lo demás. El fixture de `error` es el de los tests del host: ningún ejemplo
# produce un error de ejecución determinista y sin red.
F=packaging/anvil-host/tests/fixtures
res=0
$A "$F/paso.yaml"          --quiet >/dev/null 2>&1; [ $? -eq 0 ] || res=1
$A ejemplos/veredicto.yaml --quiet >/dev/null 2>&1; [ $? -eq 1 ] || res=1
$A "$F/error_runtime.yaml" --quiet >/dev/null 2>&1; [ $? -eq 1 ] || res=1
$A no-existe-de-verdad.yaml        >/dev/null 2>&1; [ $? -eq 1 ] || res=1
check EXIT-1 "exit 0 sólo con veredicto paso; fallo/error/carga salen 1" $res

echo "===================================================================="
echo "  OK: $ok    FALLA: $falla"
[ "$falla" -eq 0 ]
