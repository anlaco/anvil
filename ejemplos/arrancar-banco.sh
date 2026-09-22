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

# El ejecutable se instala una vez (ADR-0046 §4), no se copia junto a cada
# carpeta de módulos. En un árbol de fuentes vale el recién construido, que es
# el que alguien que está tocando el puente quiere probar; si no, el instalado.
anvil_home="${ANVIL_HOME:-$HOME/.anvil}"
exe=""
for sufijo in "" ".exe"; do          # Git Bash en Windows (ADR-0036)
    for candidato in \
        "$raiz/executors/wasm/target/release/anvil-exec-wasm$sufijo" \
        "$raiz/executors/wasm/target/debug/anvil-exec-wasm$sufijo" \
        "$anvil_home/executors/wasm/anvil-exec-wasm$sufijo"
    do
        if [ -x "$candidato" ]; then exe="$candidato"; break 2; fi
    done
done

if [ -z "$exe" ]; then
    echo "no hay ejecutor wasm en $anvil_home/executors/wasm/." >&2
    echo "  desde el paquete descargado:  ./executors/install.sh" >&2
    echo "  desde el árbol de fuentes:    make build && make install-executors" >&2
    exit 1
fi

if [ ! -f "$dist/multimetro.wasm" ]; then
    echo "no están los módulos del banco de demo en $dist — constrúyelos con 'make example'" >&2
    exit 1
fi

echo "banco de demo en 127.0.0.1:9101 (Ctrl-C para pararlo)" >&2
exec "$exe" --modules "$dist" --port 9101
