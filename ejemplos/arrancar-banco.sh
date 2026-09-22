#!/usr/bin/env bash
# Levanta el banco de demostración que usan los ejemplos.
#
# Desde ADR-0046 nada arranca un ejecutor por ti: una secuencia dice dónde hay
# uno escuchando y quien corre el banco lo pone ahí. En producción lo hace el
# arranque de la máquina o un gestor de servicios; aquí, esto.
#
#   ./ejemplos/arrancar-banco.sh &
#   ./anvil ejemplos/subsecuencia.yseq
#
# El puerto es el 9101 porque es el que declaran los `ejemplos/*.yseq`. Para
# apuntar una secuencia a otro sitio sin tocarla:
#
#   anvil ejemplos/basica.yseq --executor demo=192.168.1.50:9101
set -euo pipefail

raiz="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist="$raiz/ejemplos/departamento/dist"
exe="$dist/anvil-exec-wasm"

if [ ! -x "$exe" ]; then
    echo "no está el banco de demo en $dist — constrúyelo con 'make example'" >&2
    exit 1
fi

echo "banco de demo en 127.0.0.1:9101 (Ctrl-C para pararlo)" >&2
exec "$exe" --modules "$dist" --port 9101
