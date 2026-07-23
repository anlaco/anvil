# rpc

El motor genérico de secuencias (Fase 2): lee una `definicion_secuencia`
(datos) y la corre invocando cada paso por gRPC — nunca con una llamada
Ana directa, ni siquiera para pasos que resultan estar escritos en Ana.
Ver `../README.md` para el porqué.

## Piezas

- `paso.proto` — el contrato: `PeticionPaso` (nombre del paso, número de
  intento) y `ResultadoPasoProto` (nombre, estado, mensaje, y tres
  campos numéricos opcionales que se omiten si están vacíos, igual que
  hace la librería oficial de protobuf — verificado contra ella).
- `paso_codec.ana` — codifica/decodifica esos dos mensajes, sobre
  `grpc/protobuf.ana`. Verificado byte a byte contra `grpcio` de Python
  antes de usarse (ver el propio archivo).
- `ejecutor_pasos.ana` — el servidor: el "adaptador" que sabe qué
  función Ana llamar para cada nombre de paso (hoy, los cuatro pasos de
  `pasos_demo.ana`). Escucha en el puerto 9100.
- `motor.ana` — el cliente: conecta una vez, y para cada paso de la
  secuencia manda la petición por un stream HTTP/2 nuevo (numeración
  impar de cliente: 1, 3, 5...), reintentando el paso hasta el número de
  veces que pida su `definicion_paso.reintentos`. `ejecuta_secuencia`
  tiene el control de flujo genérico: Setup, Main solo si Setup salió
  bien (y se corta en el primer paso que falla), Cleanup siempre.

## Límite de lenguaje que esto expone

Los resultados que llegan por red vienen como **listas de bytes**, no
como texto Ana (`bytes de "texto"` no tiene inverso —
[anlaco/anlaco-lang#6](https://github.com/anlaco/anlaco-lang/issues/6)).
Consecuencia práctica: `ejecutor.reporte`/`estado_de` (pensados para
resultados de la Fase 1, en texto) revientan si se les pasa un
resultado que llegó por red — comparan `.estado` con texto vía `es`, y
comparar una lista de bytes con texto es un error de tipos en v0.4. Por
eso `motor.ana` trae su propia `estado_de_remoto`/`reporte_remoto`, que
hacen lo mismo pero comparando con `protobuf.coincide`. El reporte que
producen muestra los campos de texto como listas de bytes — no hay
forma de evitarlo hasta que se resuelva el issue de arriba.

## Otros límites encontrados construyendo esto

- Un módulo cargado con ruta de subcarpeta no puede depender de un
  módulo hermano por nombre corto —
  [anlaco/anlaco-lang#9](https://github.com/anlaco/anlaco-lang/issues/9).
  Por eso todo aquí corre desde la raíz del repo con rutas completas en
  cada `usa` (ver `../README.md` y `../../grpc/README.md`).
- `bin/anac compilar` diverge de `bin/anac ejecutar` para ese mismo
  patrón de imports —
  [anlaco/anlaco-lang#10](https://github.com/anlaco/anlaco-lang/issues/10).
  `ejecutar` es la referencia aquí.
- No hay `break` para cortar un `para cada` antes de tiempo —
  [anlaco/anlaco-lang#7](https://github.com/anlaco/anlaco-lang/issues/7).
  El "saltar el resto de Main si un paso falla" de `ejecuta_secuencia`
  usa una bandera booleana revisada en cada vuelta en su lugar.

## Cómo probarlo

```bash
bin/anac ejecutar secuenciador/rpc/ejecutor_pasos.ana &
bin/anac ejecutar secuenciador/ejemplos/basica_datos.ana
```
