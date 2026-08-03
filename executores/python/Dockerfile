# Dockerfile del LID (Legacy Isolation Domain) — ejecutor Python aislado.
#
# Imagen del ejecutor de pasos en Python (ADR-0012) que corre dentro de un
# contenedor: el LID. El contenedor solo puede salir por las puertas
# declaradas (reglas de firewall del host); el motor de Anvil lo ve como un
# endpoint gRPC en una IP.
#
# Construir:
#   docker build -t anvil-lid-python .
#
# Correr (ver docker-compose.yml para la red limitada):
#   docker run --rm -p 9101:9101 anvil-lid-python
#   docker run --rm -p 9101:9101 anvil-lid-python --simulador 192.168.1.50:4000

FROM python:3.12-slim

WORKDIR /lid

# Stubs generados desde paso.proto. Se regeneran con:
#   python3 -m grpc_tools.protoc -I ../../crates/modelo \
#     --python_out=. --grpc_python_out=. ../../crates/modelo/paso.proto
# Aquí los copiamos ya generados para que la imagen sea autocontenida.
COPY paso_pb2.py paso_pb2_grpc.py ./
COPY server.py simulador_tcp.py ./

# Solo grpcio en runtime (los stubs ya están generados).
RUN pip install --no-cache-dir grpcio

EXPOSE 9101

# Por defecto escucha en 0.0.0.0:9101 (acepta conexiones del host de Anvil).
# El simulador por defecto es 127.0.0.1:4000; se override con --simulador.
CMD ["python3", "server.py", "--puerto", "9101"]