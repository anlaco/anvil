#!/usr/bin/env python3
"""Ejecutor de pasos en Python (ADR-0012) — módulo distribuido de Anvil.

Servidor gRPC que habla el contrato `paso.proto` (service EjecutorPasos,
ruta /EjecutorPasos/Invoca). El motor de Anvil lo ve como un endpoint más:
despacha por nombre→endpoint y no sabe que detrás hay Python.

Este ejecutor demuestra la pieza clave del escenario legacy: el paso que
toca hardware no vive en el SO de Anvil. Aquí, los pasos "de instrumento"
hablan por TCP con un simulador de instrumento (el que está desarrollando
otro equipo). En producción, ese destino TCP puede ser una caja con el
driver del fabricante en un Windows 7 (LID), una VM, o un simulador.

Uso:
    python3 server.py                 # escucha en 127.0.0.1:9101
    python3 server.py --puerto 9200   # otro puerto
    python3 server.py --simulador 192.168.1.50:4000  # LID: caja legacy

El simulador TCP se configura con --simulador; por defecto apunta a
127.0.0.1:4000. Para probar sin el simulador del otro equipo, arranca
`simulador_tcp.py` (un fake mínimo) en otra terminal.
"""

import argparse
import socket
import sys
from concurrent import futures

import grpc

import paso_pb2
import paso_pb2_grpc

HOST = "127.0.0.1"
PUERTO = 9101
SIMULADOR_DEFECTO = ("127.0.0.1", 4000)


def lee_simulador(host, puerto, comando, timeout=2.0):
    """Una petición al simulador: abre TCP, envía `comando`, lee la línea.

    El protocolo con el simulador es deliberadamente trivial (línea de
    texto, respuesta `medida: <valor>` o `ok`). Cuando el equipo del
    simulador cierre el contrato real, esto se sustituye por su cliente
    sin tocar nada más del ejecutor: el paso solo devuelve
    ResultadoPasoProto.
    """
    with socket.create_connection((host, puerto), timeout=timeout) as sock:
        sock.sendall((comando + "\n").encode("utf-8"))
        linea = sock.recv(4096).decode("utf-8").strip()
    return linea


class EjecutorPasosServicer(paso_pb2_grpc.EjecutorPasosServicer):
    """Despacho por nombre: el único punto donde el nombre del cable se ata
    a una función de este ejecutor (igual que pasos_demo en el ejecutor
    WASM). Un nombre desconocido es `error`, no excepción (RF-12).
    """

    def __init__(self, simulador):
        self.simulador = simulador

    def Invoca(self, request, context):
        nombre = request.nombre
        intento = request.intento

        if nombre == "medir_simulador":
            # Mide contra el simulador: comando = "medir", respuesta
            # "medida: <valor>". El límite lo evalúa el motor desde el YAML
            # (ADR-0008); aquí solo se devuelve la medida.
            try:
                linea = lee_simulador(*self.simulador, "medir")
            except OSError as e:
                return paso_pb2.ResultadoPasoProto(
                    nombre=nombre,
                    estado="error",
                    mensaje=f"no se pudo hablar con el simulador: {e}",
                )
            if linea.lower().startswith("medida:"):
                return paso_pb2.ResultadoPasoProto(
                    nombre=nombre,
                    estado="paso",
                    mensaje=f"simulador respondió {linea}",
                    valor_medido=linea.split(":", 1)[1].strip(),
                )
            return paso_pb2.ResultadoPasoProto(
                nombre=nombre,
                estado="error",
                mensaje=f"respuesta ilegible del simulador: {linea!r}",
            )

        if nombre == "conectar_equipo":
            # Mismo patrón que pasos_demo::conectar: fallo transitorio en el
            # intento 1, pasa desde el 2 (RF-09: el número de intento llega
            # al paso).
            if intento == 1:
                return paso_pb2.ResultadoPasoProto(
                    nombre=nombre, estado="fallo",
                    mensaje="handshake del simulador perdido (transitorio)",
                )
            return paso_pb2.ResultadoPasoProto(
                nombre=nombre, estado="paso", mensaje="conectado",
            )

        if nombre == "verificar_led":
            # Pass/fail sin medida (built-in de ejemplo en Python).
            return paso_pb2.ResultadoPasoProto(
                nombre=nombre, estado="paso", mensaje="led encendido",
            )

        return paso_pb2.ResultadoPasoProto(
            nombre=nombre, estado="error",
            mensaje=f"paso no reconocido por el ejecutor python: {nombre}",
        )


def main():
    parser = argparse.ArgumentParser(description="Ejecutor de pasos en Python (ADR-0012)")
    parser.add_argument("--puerto", type=int, default=PUERTO)
    parser.add_argument("--simulador", default=None,
                        help="host:puerto del simulador TCP (p. ej. 192.168.1.50:4000)")
    args = parser.parse_args()

    simulador = SIMULADOR_DEFECTO
    if args.simulador:
        host, _, puerto = args.simulador.partition(":")
        simulador = (host, int(puerto))

    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    paso_pb2_grpc.add_EjecutorPasosServicer_to_server(
        EjecutorPasosServicer(simulador), server)
    server.add_insecure_port(f"{HOST}:{args.puerto}")
    server.start()
    print(f"ejecutor python escuchando en {HOST}:{args.puerto} "
          f"(simulador en {simulador[0]}:{simulador[1]})")
    sys.stdout.flush()
    server.wait_for_termination()


if __name__ == "__main__":
    main()
