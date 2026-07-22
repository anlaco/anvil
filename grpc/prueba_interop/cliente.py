import grpc
import saludador_pb2
import saludador_pb2_grpc

def main():
    channel = grpc.insecure_channel("127.0.0.1:9099")
    stub = saludador_pb2_grpc.SaludadorStub(channel)
    respuesta = stub.Saluda(saludador_pb2.SaludoRequest(nombre="anvil"), timeout=8)
    print("Respuesta:", respuesta.mensaje)

if __name__ == "__main__":
    main()
