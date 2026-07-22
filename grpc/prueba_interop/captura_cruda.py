import socket

HOST, PORT = "127.0.0.1", 9099


def frame_header(length, ftype, flags, stream_id):
    return length.to_bytes(3, "big") + bytes([ftype, flags]) + (stream_id & 0x7FFFFFFF).to_bytes(4, "big")


with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((HOST, PORT))
    s.listen(1)
    print(f"escuchando en {PORT}...")
    conn, addr = s.accept()
    print("conexión de", addr)
    conn.settimeout(6.0)

    datos = b""
    try:
        while True:
            trozo = conn.recv(4096)
            if not trozo:
                break
            datos += trozo
    except socket.timeout:
        pass

    print(f"capturados (antes de contestar) {len(datos)} bytes")

    # Contestamos como un servidor HTTP/2 mínimo: nuestro SETTINGS vacío +
    # ACK del SETTINGS del cliente. Igual que hace servidor_handshake.ana.
    conn.sendall(frame_header(0, 4, 0, 0))  # SETTINGS vacío
    conn.sendall(frame_header(0, 4, 1, 0))  # SETTINGS ACK

    try:
        while True:
            trozo = conn.recv(4096)
            if not trozo:
                break
            datos += trozo
    except socket.timeout:
        pass
    conn.close()

print(f"capturados {len(datos)} bytes en total")
with open("captura.bin", "wb") as f:
    f.write(datos)
print("guardado en captura.bin")
