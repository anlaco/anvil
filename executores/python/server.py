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

# La versión de contrato que habla este ejecutor (ADR-0020 §4).
#
# Tiene que coincidir con `modelo::proto::CONTRATO` del core. El motor
# comprueba este número —el "eco"— y convierte el paso en `error` si un
# ejecutor que recibe `parametros` responde uno menor: un ejecutor que ignora
# los parámetros **mide otra cosa y dice `paso`**, y ese verde falso es lo
# único que este número existe para impedir.
#
# Si subes el contrato en el core y te olvidas de aquí, este ejecutor deja de
# poder correr pasos con parámetros — que es el fallo ruidoso y correcto.
CONTRATO = 2


def valor_a_python(v):
    """Un `Valor` del cable al tipo de Python que le corresponde.

    Devuelve `None` si el `oneof` llegó sin rama: no dice de qué tipo es, y
    ejecutar el paso sin ese parámetro sería medir otra cosa en silencio
    (ADR-0019, Regla 2). Ojo: `None` aquí significa "no interpretable", no
    "ausente" — un parámetro ausente sencillamente no está en la lista.
    """
    cual = v.WhichOneof("valor")
    if cual is None:
        return None
    return getattr(v, cual)


def parametros_de(request):
    """Los parámetros de la petición como `dict`, o `(None, nombre)` si uno
    llegó sin tipo."""
    fuera = {}
    for v in request.parametros:
        valor = valor_a_python(v)
        if valor is None:
            return None, v.nombre
        fuera[v.nombre] = valor
    return fuera, None


def resultado(nombre, estado, mensaje, valor_medido=None, salidas=None):
    """Construye la respuesta **con el eco del contrato siempre puesto**.

    Existe para que no haya forma de devolver un resultado sin eco: con 6
    puntos de retorno, olvidarse en uno solo sería un ejecutor que a veces
    dice que entiende el contrato y a veces no.
    """
    r = paso_pb2.ResultadoPasoProto(
        nombre=nombre, estado=estado, mensaje=mensaje, contrato=CONTRATO,
    )
    if valor_medido is not None:
        r.valor_medido = str(valor_medido)
    for n, v in (salidas or {}).items():
        s = r.salidas.add()
        s.nombre = n
        if isinstance(v, bool):
            s.booleano = v
        elif isinstance(v, (int, float)):
            s.numero = float(v)
        else:
            s.texto = str(v)
    return r


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
        params, sin_tipo = parametros_de(request)
        if sin_tipo is not None:
            return resultado(
                nombre, "error",
                f"el parámetro '{sin_tipo}' llegó sin tipo (ninguna de las "
                f"ramas numero/texto/booleano): el paso no puede saber con "
                f"qué medir")

        if nombre == "medir_simulador":
            # Mide contra el simulador: comando = "medir", respuesta
            # "medida: <valor>". El límite lo evalúa el motor desde el YAML
            # (ADR-0008); aquí solo se devuelve la medida.
            # ADR-0020: el canal ya no está grabado en el ejecutor. Sin
            # parámetro, el canal 1 y el mismo comando de siempre.
            canal = params.get("canal", 1)
            comando = "medir" if canal == 1 else f"medir {canal}"
            try:
                linea = lee_simulador(*self.simulador, comando)
            except OSError as e:
                return resultado(
                    nombre, "error", f"no se pudo hablar con el simulador: {e}")
            if linea.lower().startswith("medida:"):
                return resultado(
                    nombre, "paso", f"simulador respondió {linea}",
                    valor_medido=linea.split(":", 1)[1].strip(),
                    # El canal usado vuelve como salida con nombre: es la
                    # condición en la que se midió, y ahora queda en el
                    # informe (ADR-0020, Regla 3 de ADR-0019).
                    salidas={"canal_usado": canal},
                )
            return resultado(
                nombre, "error", f"respuesta ilegible del simulador: {linea!r}")

        if nombre == "conectar_equipo":
            # Mismo patrón que pasos_demo::conectar: fallo transitorio en el
            # intento 1, pasa desde el 2 (RF-09: el número de intento llega
            # al paso).
            if intento == 1:
                return resultado(
                    nombre, "fallo",
                    "handshake del simulador perdido (transitorio)")
            return resultado(nombre, "paso", "conectado")

        if nombre == "verificar_led":
            # Pass/fail sin medida (built-in de ejemplo en Python).
            return resultado(nombre, "paso", "led encendido")

        return resultado(
            nombre, "error",
            f"paso no reconocido por el ejecutor python: {nombre}")


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
