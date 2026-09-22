#!/bin/sh
# Instala los ejecutores de este paquete donde un `runtime` lógico se resuelve
# (ADR-0046 §4): `~/.anvil/executors/`, o `$ANVIL_HOME/executors`.
#
# Se copia y ya. No hay registro, ni servicio, ni nada que arrancar: un
# ejecutor se instala una vez y quien lo levanta es el arranque de la máquina,
# un gestor de servicios, o una persona — nunca el motor, y nunca una secuencia.
#
#   ./executors/install.sh
#   ANVIL_HOME=/opt/anvil ./executors/install.sh
set -eu

aqui="$(cd "$(dirname "$0")" && pwd)"
destino="${ANVIL_HOME:-$HOME/.anvil}/executors"

mkdir -p "$destino"
for runtime in "$aqui"/*/; do
    nombre="$(basename "$runtime")"
    [ -f "$runtime/executor.json" ] || continue
    mkdir -p "$destino/$nombre"
    cp -R "$runtime." "$destino/$nombre/"
    echo "  $nombre → $destino/$nombre"
done

echo "ejecutores instalados en $destino"
