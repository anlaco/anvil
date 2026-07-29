#!/usr/bin/env bash
# Verifica que la migración a Rust+WASM conserva la especificación: corre la
# secuencia "basica_datos" en las cuatro combinaciones de motor y ejecutor, y
# exige que las cuatro salidas sean idénticas.
#
#   1. Ana   -> Ana    (la referencia; requiere bin/anac)
#   2. Rust  -> Rust   (la migración)
#   3. Rust  -> Ana    (el motor nuevo contra el ejecutor viejo)
#   4. Ana   -> Rust   (el motor viejo contra el ejecutor nuevo)
#
# Las dos cruzadas son las que de verdad prueban algo: que el contrato gRPC
# se respeta byte a byte y que las dos implementaciones son intercambiables.
#
# Correr desde la raíz del repo:  ./verifica_paridad.sh
# Si no está bin/anac, las rondas con Ana se saltan y se compara contra la
# salida esperada empotrada aquí abajo.

set -uo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM="$RAIZ/target/wasm32-wasip2/debug"
WASMTIME_FLAGS=(-S cli -S tcp=y -S inherit-network=y)
PUERTO=9100
TMP="$(mktemp -d)"
FALLOS=0
SRV=""

# La salida que fija la spec. El Main corta en medir_voltaje, así que
# verificar_led no llega a correr — eso es correcto, no un paso perdido.
ESPERADO="$TMP/esperado.txt"
cat > "$ESPERADO" <<'EOF'
conectado al ejecutor de pasos
=== basica_datos: fallo ===
  [paso] conectar_equipo: equipo conectado (intento 2)
  [fallo] medir_voltaje: voltaje fuera de rango
  [paso] desconectar_equipo: equipo desconectado
EOF

limpia() {
  [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
  rm -rf "$TMP"
}
trap limpia EXIT

hay_ana() { [ -x "$RAIZ/bin/anac" ]; }

# Un servidor huérfano de una corrida previa atendería las llamadas y las
# rondas pasarían sin ejercitar lo que creemos estar probando.
exige_puerto_libre() {
  if ss -ltn 2>/dev/null | grep -q ":$PUERTO "; then
    echo "ERROR: el puerto $PUERTO ya está ocupado:"
    ss -ltnp 2>/dev/null | grep ":$PUERTO "
    exit 1
  fi
}

# Espera leyendo el log, no sondeando el puerto: el ejecutor acepta una sola
# conexión, así que un sondeo se comería la del motor.
espera_servidor() {
  local log="$1"
  for _ in $(seq 1 75); do
    grep -q "escuchando" "$log" 2>/dev/null && return 0
    if ! kill -0 "$SRV" 2>/dev/null; then
      echo "  el ejecutor murió al arrancar:"; sed 's/^/    /' "$log"; return 1
    fi
    sleep 0.2
  done
  echo "  el ejecutor no arrancó en 15s:"; sed 's/^/    /' "$log"; return 1
}

# Mata el grupo de procesos entero, no solo el shell: `bin/anac ejecutar`
# lanza un hijo que se queda con el puerto si solo se mata al padre, y un
# huérfano con el 9100 haría pasar las rondas siguientes sin ejercitar nada.
para_servidor() {
  [ -z "$SRV" ] && return
  kill -- "-$SRV" 2>/dev/null || kill "$SRV" 2>/dev/null
  wait "$SRV" 2>/dev/null
  SRV=""
  for _ in $(seq 1 25); do
    ss -ltn 2>/dev/null | grep -q ":$PUERTO " || break
    sleep 0.2
  done
}

# ronda <etiqueta> <arranca-ejecutor> <corre-motor>
ronda() {
  local etiqueta="$1" ejecutor="$2" motor="$3"
  local log="$TMP/${etiqueta// /_}.srv" salida="$TMP/${etiqueta// /_}.out"
  echo
  echo "=== $etiqueta ==="

  exige_puerto_libre
  # setsid: el ejecutor va en su propio grupo de procesos para poder matarlo
  # entero (ver para_servidor).
  setsid bash -c "$ejecutor" > "$log" 2>&1 &
  SRV=$!
  if ! espera_servidor "$log"; then
    FALLOS=$((FALLOS + 1)); para_servidor; return
  fi

  if ! timeout 30 bash -c "$motor" > "$salida" 2>&1; then
    echo "  -> FALLO: el motor salió con error"; sed 's/^/    /' "$salida"
    FALLOS=$((FALLOS + 1)); para_servidor; return
  fi
  para_servidor

  if diff -u "$ESPERADO" "$salida" > "$TMP/diff.txt"; then
    echo "  -> OK (salida idéntica a la spec)"
  else
    echo "  -> FALLO: la salida difiere de la spec"
    sed 's/^/    /' "$TMP/diff.txt"
    FALLOS=$((FALLOS + 1))
  fi
}

echo "Compilando para wasm32-wasip2..."
(cd "$RAIZ" && cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor) || exit 1

EJ_RUST="wasmtime ${WASMTIME_FLAGS[*]} $WASM/ejecutor_pasos.wasm"
MO_RUST="wasmtime ${WASMTIME_FLAGS[*]} $WASM/basica_datos.wasm"
EJ_ANA="cd $RAIZ && bin/anac ejecutar secuenciador/rpc/ejecutor_pasos.ana"
MO_ANA="cd $RAIZ && bin/anac ejecutar secuenciador/ejemplos/basica_datos.ana"

if hay_ana; then
  ronda "1. Ana -> Ana (referencia)" "$EJ_ANA" "$MO_ANA"
else
  echo; echo "(sin bin/anac: se saltan las rondas con Ana, se compara contra la spec empotrada)"
fi

ronda "2. Rust -> Rust (la migración)" "$EJ_RUST" "$MO_RUST"

if hay_ana; then
  ronda "3. Rust -> Ana (motor nuevo, ejecutor viejo)" "$EJ_ANA" "$MO_RUST"
  ronda "4. Ana -> Rust (motor viejo, ejecutor nuevo)" "$EJ_RUST" "$MO_ANA"
fi

echo
if [ "$FALLOS" -eq 0 ]; then
  echo "Paridad verificada: todas las combinaciones dan la misma salida."
else
  echo "$FALLOS ronda(s) fallaron."
fi
exit "$FALLOS"
