#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""Simulador de instrumento TCP mínimo (stand-in de desarrollo).

Mientras el equipo del simulador real no cierra su contrato, este fake
responde a `medir` con un valor dentro del rango de la demo. Arrancarlo
en otra terminal junto a `server.py`:

    python3 simulador_tcp.py

Protocolo: línea de texto de entrada, línea de texto de salida.
- `medir`    -> `medida: 4.8`
- `reset`    -> `ok`
- otra cosa  -> `error: comando desconocido`

La conexión **se mantiene abierta** hasta que el cliente la cierra, y admite
varias líneas seguidas. Eso es lo que permite que un paso abra una sesión con
el banco y la reutilice en los pasos siguientes (ADR-0022): un socket abierto
es justo lo que no puede cruzar el cable, y por eso lo que cruza es una
referencia.
"""

import socket

HOST = "127.0.0.1"
PUERTO = 4000


def responde(linea):
    linea = linea.strip().lower()
    if linea == "medir":
        return "medida: 4.8"
    if linea == "reset":
        return "ok"
    return "error: comando desconocido"


def main():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind((HOST, PUERTO))
        srv.listen(1)
        print(f"simulador tcp escuchando en {HOST}:{PUERTO}")
        while True:
            conn, _ = srv.accept()
            with conn:
                # Una línea por petición, hasta que el cliente cierre. Antes se
                # atendía una sola y se colgaba: una sesión que sobrevive a
                # varios pasos manda varias.
                while True:
                    datos = conn.recv(4096)
                    if not datos:
                        break
                    for linea in datos.decode("utf-8").splitlines():
                        if linea.strip():
                            conn.sendall((responde(linea) + "\n").encode("utf-8"))


if __name__ == "__main__":
    main()
