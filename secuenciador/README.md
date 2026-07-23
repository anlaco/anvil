# secuenciador

El motor de secuencia de test de anvil, en Ana.

## Estructura

- `modelo.ana` — tarjetas de datos: `resultado_step`/`resultado_secuencia`
  (lo que YA corrió) y `definicion_paso`/`definicion_secuencia` (lo que
  HAY que correr — la entrada del motor genérico).
- `ejecutor.ana` — bookkeeping: `registra` (añade un resultado a una
  secuencia), `estado_de`/`reporte` (agregan y muestran resultados de
  TEXTO Ana — no usar con resultados que llegaron por red, ver abajo).
- `pasos_demo.ana` — los pasos de la secuencia de ejemplo
  (`conectar_equipo`, `medir_voltaje`, `verificar_led`,
  `desconectar_equipo`), compartidos entre la Fase 1 y el ejecutor de
  pasos por gRPC.
- `ejemplos/basica.ana` — **Fase 1**: la secuencia corrida en Ana puro,
  sin red, con el control de flujo (reintentos, saltar Main si falla
  Setup, Cleanup siempre) escrito a mano en el propio script.
- `ejemplos/basica_datos.ana` + `rpc/` — **Fase 2**: la misma secuencia,
  pero como DATOS (`definicion_secuencia`) corrida por un motor
  genérico que invoca cada paso por gRPC, nunca con una llamada Ana
  directa. Ver `rpc/README.md`.

## Por qué dos fases

Ana no tiene funciones de primera clase — no hay forma de pasarle al
motor, como dato, "qué función Ana llamar" para un paso con ese nombre.
Por eso el control de flujo genérico (Fase 2) solo puede compartirse de
verdad si el despacho por nombre pasa por algo que SÍ es
dato-orientado: una llamada de red. Es también la decisión correcta
para el objetivo del proyecto — todo paso (Ana, Python, una DLL más
adelante) se invoca igual, aislado en su propio proceso.

## Cómo correr

Todo desde la raíz del repo (ver `../grpc/README.md` sobre por qué):

```bash
# Fase 1 — sin red:
bin/anac ejecutar secuenciador/ejemplos/basica.ana

# Fase 2 — con red, dos procesos:
bin/anac ejecutar secuenciador/rpc/ejecutor_pasos.ana &
bin/anac ejecutar secuenciador/ejemplos/basica_datos.ana
```

Los dos dan el mismo resultado (mismo fallo simulado en
`medir_voltaje`, mismo salto de `verificar_led`, mismo Cleanup) — la
Fase 2 lo muestra con los campos de texto como listas de bytes en vez
de palabras limpias, porque Ana no tiene conversión de bytes a texto
todavía (`ejecutor.reporte` no sirve para resultados que llegaron por
red por eso mismo — usar `rpc/motor.ana:reporte_remoto`, ver
`rpc/README.md`).
