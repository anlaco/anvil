# Informe de beta — Anvil 0.1.0 (agosto 2026)

> **Alcance:** hallazgos de la primera campaña de betatesting externa, verificados
> contra el binario y anclados al código del repo. **4 defectos** con reproducción
> mínima, **5 mejoras de diagnóstico** y **2 carencias de trazabilidad** del
> reporte. Nada de esto afecta al núcleo (motor de expresiones, evaluación de
> límites, máquina de estados, sequence call), que salió indemne.

**Base:** binario `anvil 0.1.0` (Linux x86_64), 180 secuencias YAML y 21
componentes WASM producidos por la campaña. Más de 600 ejecuciones sin un solo
cuelgue, crash ni resultado no determinista.

**Reproducciones:** [`regresion/`](regresion/) — `./docs/qa/regresion/run.sh`
afirma el comportamiento **correcto**, así que hoy salen los 6 casos en `FALLA` y
pasarán a `OK` a medida que se arreglen.

---

## 1. Resumen

Lo que la campaña ejercitó de verdad —límites embebidos y sidecar, variables en
tres scopes, precondiciones, statements, control de flujo, subsecuencias inline y
externas, parameters by-reference hasta 3 niveles, process models, hasta 7
ejecutores WASM simultáneos, exportación JSON— funciona y es reproducible.

Los defectos están en el **perímetro**, y comparten un patrón que conviene mirar
de frente: **una construcción mal escrita no produce error, produce un resultado
silenciosamente incorrecto**. En un secuenciador de test es la clase de fallo más
caro, porque no rompe el pipeline: contamina los datos y el verde se mantiene.

Orden de arreglo recomendado:

| # | Qué | Severidad | Coste estimado |
|---|---|---|---|
| 1 | [DEF-1](#def-1) — `--limits` no llega a la secuencia del operador bajo `--process-model` | alta | bajo |
| 2 | [DIAG-1](#diag-1) — avisar cuando el sidecar afecta a 0 pasos | — | trivial |
| 3 | [DEF-3](#def-3) — `asigna` ensombrece en silencio un `parameter` declarado | media (produce verdes falsos) | bajo |
| 4 | [DIAG-2](#diag-2) — `statement` no puede expresar un veredicto que falle | — | medio |
| 5 | [DEF-2](#def-2) — la columna `nombre_secuencia` del CSV lleva el estado | media | trivial |

DIAG-1 va tan arriba a propósito: es un aviso de dos líneas que habría delatado
DEF-1 el primer día en lugar de a las 180 secuencias.

---

## 2. Defectos

### DEF-1 — `--limits` se aplica al process model, no a la secuencia del operador
<a id="def-1"></a>
**Severidad: alta.** Reproducción: [`regresion/bug1-sidecar-pm.md`](regresion/bug1-sidecar-pm.md)

Con ficheros del propio repo:

```sh
# A) sin PM: el sidecar se aplica y la secuencia pasa
anvil ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml
#   sidecar de límites '...' aplicado (1 paso(s) afectado(s))
#   === limites: paso ===

# B) con PM: el mismo sidecar afecta a 0 pasos y la secuencia falla
anvil --process-model ejemplos/process_model_sequential.yaml \
      ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml
#   sidecar de límites '...' aplicado (0 paso(s) afectado(s))
#   === process_model_sequential: fallo ===
#     [fallo] medir_voltaje: 4.2 fuera de rango [4.5, 5.5]
```

**Causa raíz.** En `crates/motor/src/bin/anvil.rs:171` el sidecar se inyecta sobre
la raíz del programa:

```rust
let n = aplicar_limites(&mut programa.raiz, &limites);
```

Con `--process-model`, `cargar_programa_con_process_model`
(`crates/cargador/src/lib.rs:560`) hace que **la raíz sea el PM** y la secuencia
del operador quede como secuencia invocada. Los nombres del sidecar
(`medir_voltaje`) no coinciden con los pasos del PM (`abrir_fixture`,
`identificar_uut`, `test_uut`, `cerrar_fixture`), así que no se sobreescribe nada.

Es la intersección de dos alcances ya declarados —el comentario en
`anvil.rs:159-162` dice que aplicar el sidecar a las subsecuencias externas es
post-MVP y que «hoy el sidecar cubre la secuencia principal»— pero el efecto
visible contradice lo que promete `docs/diseno/proceso-de-test.md` y el manual:
que la misma secuencia va de I&D a fábrica cambiando solo el process model. El
mecanismo declarado para variar umbrales por lote deja de funcionar exactamente
en el modo de producción, y lo hace **en silencio**.

**Opciones de arreglo**, de menor a mayor alcance:

1. Aplicar el sidecar a la secuencia de usuario cuando hay PM (resolver el paso
   con `secuencia_usuario: true` y aplicar `aplicar_limites` sobre su definición).
   Es lo que espera el usuario y es local a `anvil.rs`.
2. Aplicarlo a la raíz **y** a las secuencias invocadas (levanta el recorte
   post-MVP para el caso del PM, que es el que importa en fábrica).
3. Como mínimo, DIAG-1: avisar de que no afectó a nada.

**Criterio de aceptación:** en el caso B, 1 paso afectado y agregado `paso`.

---

### DEF-2 — la columna `nombre_secuencia` del CSV contiene el estado
<a id="def-2"></a>
**Severidad: media.** Reproducción: [`regresion/bug2-csv-nombre.yaml`](regresion/bug2-csv-nombre.yaml)

```csv
nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,...
paso,paso,verificar_led,paso,led encendido,,,,,
paso,paso,abrir_rele,paso,relé abierto,,,,,
```

La primera columna repite el agregado; el nombre de la secuencia **no aparece en
ninguna columna**. Un CSV concatenado de varias secuencias es inutilizable: no se
puede saber de qué secuencia es cada fila. El sink JSON sí lo emite bien.

**Causa raíz.** `crates/result_sink/src/csv.rs:78` — `fila_paso` recibe
`estado_secuencia` pero nunca el nombre, y lo emite como primer campo:

```rust
fn fila_paso(estado_secuencia: &str, p: &ResultadoStep, nombre: &str) -> Vec<String> {
    vec![
        estado_secuencia.to_string(),   // ← columna "nombre_secuencia"
        p.estado.clone(),               //   columna "estado"
        …
```

`ResultadoSecuencia` ya trae el nombre en `on_fin_secuencia`, así que el arreglo
es propagarlo.

**Atención:** el test unitario de `csv.rs:151` **afirma la salida incorrecta**
(`"fallo,fallo,medir_voltaje,…"`), así que el defecto está fijado por un test y
hay que corregir ambos. Es también la explicación de por qué sobrevivió: la
campaña no usó `--csv` ni una vez (ver §4).

---

### DEF-3 — `asigna` ensombrece en silencio un `parameter` declarado
<a id="def-3"></a>
**Severidad: media**, alta por consecuencias: produce verdes falsos.
Reproducción: [`regresion/bug3-sub-asigna-parameter.yaml`](regresion/bug3-sub-asigna-parameter.yaml)

`asigna` escribe siempre en Locals (ADR-0009). Si el destino coincide con el
nombre de un `parameter` declarado, el motor no avisa: crea un local nuevo, el
parameter conserva su valor inicial y el retorno by-reference devuelve **ese valor
inicial** al padre.

```yaml
parameters:
  p: 0.0
main:
  - nombre: medir_voltaje                          # mide 4.2
    asigna: { p: '${resultado.valor_medido}' }     # escribe locals.p, NO parameters.p
  - nombre: verificar_led
    precondicion: 'parameters.p > 4.0'             # → [saltado]: sigue a 0.0
  - nombre: abrir_rele
    precondicion: 'locals.p > 4.0'                 # → [paso]: el shadow vale 4.2
```

Es un footgun de primera magnitud porque **las tres señales que tendría un
ingeniero fallan a la vez**: el YAML es válido, el cargador no dice nada, y la
secuencia reporta `paso`. En el corpus de la beta apareció en 3 ficheros; en uno
hacía que se saltaran 3 de 7 pasos y el test seguía en verde.

**Causa raíz.** `crates/motor/src/lib.rs:583` — `ent.escribe(Scope::Locals, &a.var, v)`
crea la local sin comprobar que esté declarada ni que el nombre choque con un
parameter.

**Arreglo recomendado, y por qué es el natural aquí:** el cargador **ya hace esta
clase de validación** para los argumentos de un sequence call
(`crates/cargador/src/lib.rs:809`), donde un `locals.X` no declarado en el padre
es `ErrorCarga::Validacion`. Extender el mismo criterio al destino de `asigna`
—declarado en `locals` y sin colisión con `parameters`— es simétrico con lo que ya
existe, fail-fast como el resto del cargador, y no toca ADR-0009.

---

### DEF-4 — un `path` absoluto de ejecutor WASM se carga y luego se rechaza
<a id="def-4"></a>
**Severidad: baja** (diagnóstico contradictorio).

```
anvil-puente-wasm: cargado '/ruta/absoluta/multimetro.wasm'
anvil-puente-wasm: escuchando en 127.0.0.1:40479
ejecutor 'dmm' cargado (/ruta/absoluta/multimetro.wasm → 127.0.0.1:40479)
no se pudo cargar la secuencia: secuencia inválida: el ejecutor 'dmm' es 'wasm'
  y su 'path' '/ruta/absoluta/multimetro.wasm' no existe
```

El puente host-side resuelve el fichero y lo carga; después el cargador —que
valida dentro del sandbox, con solo el CWD preabierto— dice que no existe. Ambas
cosas no pueden ser verdad. El mensaje debería explicar que los paths absolutos
quedan fuera del directorio preabierto, no que el fichero no existe.

---

## 3. Diagnóstico y trazabilidad

### DIAG-1 — avisar cuando el sidecar afecta a 0 pasos
<a id="diag-1"></a>
**La mejora más rentable del informe.** Hoy `anvil.rs:172` emite
`sidecar de límites 'x.yaml' aplicado (0 paso(s) afectado(s))` como línea
informativa perdida en stderr. Como aviso explícito —«el sidecar no afectó a
ningún paso: comprueba que los nombres coincidan»— delata DEF-1 de inmediato, y
también los sidecars con nombres de paso mal escritos, que en la campaña costaron
varias secuencias.

### DIAG-2 — un `statement` que evalúa `false` no puede fallar el paso
<a id="diag-2"></a>
Los statements solo admiten asignación. El patrón que sale de ahí es que el
veredicto compuesto de la secuencia se asigne a un local que nadie vuelve a leer:
**131 de las 180 secuencias de la beta terminan en un `eval_final` / `dut_ok` /
«índice de calidad global ponderado» que no puede hacer fallar nada**, y solo 2
vuelven a consumir ese local. Es decorativo, y explica buena parte de por qué esa
suite da verde casi siempre.

Propuesta: `statement` con expresión booleana sin asignación → `paso` si `true`,
`fallo` si `false` (un *assert*). Sin esto no hay forma de expresar en Anvil un
criterio de aceptación que combine varias medidas, que es justo lo que un
ingeniero de test quiere al final de una secuencia. Los betatesters llegaron a
escribir `locals.x = locals.x` para rellenar.

### DIAG-3 — el reporte y el JSON no indican la fase
Las claves por paso son `estado, limite_max, limite_min, mensaje, nombre,
operador, valor_esperado, valor_medido`. No hay `fase`, así que al post-procesar
no se distingue un fallo de Setup de uno de Main o Cleanup — una distinción de
primer orden para un `ResultSink` y para triar en fábrica. Ver
`docs/diseno/reportes.md`.

### DIAG-4 — con `--process-model` se pierde qué secuencia se ejecutó

```
=== process_model_sequential: fallo ===
  [fallo] test_uut: sequence call '__anvil_usuario__' → fallo
```

El nombre de la secuencia del operador (`basica`) no aparece en ninguna parte del
reporte. En producción el PM es obligatorio, así que el informe **no registra qué
test se corrió**. Sustituir el placeholder por el nombre real, o añadirlo al JSON,
es trazabilidad básica.

### DIAG-5 — mensajes que apunten al campo correcto

| Escrito | Mensaje actual | Sugerencia |
|---|---|---|
| sidecar con envoltorio `limites:` / `limite:` | `unknown field: medir_voltaje_dc` | «el sidecar es un mapa plano paso→límite; no admite campo envoltorio» |
| `steps:` en una subsecuencia inline | `unknown field: steps` | «¿querías `main:`?» |
| `--flagquenoexiste` | `el flag '--flagquenoexiste' necesita un valor` | «flag desconocido» |
| `.wasm` que es módulo core, no componente | `componente inválido: failed to parse WebAssembly module` | «es un módulo core, no un componente: ¿falta `bindings::export!`?» |

Los cuatro costaron tiempo real en la campaña. El primero produjo un bug fantasma
(un sidecar mal formado les hizo concluir que «el sidecar no funciona con process
model», que era falso *por ese motivo*, aunque DEF-1 resultara real por otro). El
último les hizo culpar al toolchain durante una tanda entera cuando lo que faltaba
era una línea en su propio componente.

### Nota — el puerto 9100 es fijo
El ejecutor embebido escucha siempre en 9100, así que **dos procesos `anvil` no
pueden coexistir**: el segundo muere con `address-in-use`. Es coherente con el MVP
(el paralelismo es post-v1), pero conviene decidirlo explícitamente: hoy impide
paralelizar una campaña grande por el método más simple, que es lanzar N procesos.
Un puerto efímero por proceso, o un `--puerto`, lo resolvería sin tocar el modelo
de ejecución.

---

## 4. Cobertura: lo que la beta no llegó a probar

De las 16 capacidades del MVP, estas quedaron sin ejercitar. Las dos primeras son
donde salieron DEF-2 y DEF-4 en cinco minutos, así que el resto merece atención:

| Capacidad | Uso en 180 secuencias |
|---|---|
| `--csv` | **0** → DEF-2 |
| Ejecutor `tipo: grpc` (endpoint externo) | **0** |
| `--ejecutor nombre=host:puerto` | **0** |
| `--solo-loopback` | **0** |
| `-h` / `-V` | **0** |
| `pause_on_fail` | 8 ficheros |
| `disable` | 15 ficheros |

Además los 21 instrumentos WASM devuelven constantes: **180 de 181 ramas retornan
`estado: "paso"`** y solo 7 de 21 varían con `intento`. El corpus casi no ejerce
los caminos de `fallo` ni de `error`, ni la lógica de reintentos.

---

## 5. La lección de producto: `resultado.*` fuera de `asigna`
<a id="leccion"></a>

De los 8 bugs que reportó la campaña, **6 no lo eran**: era semántica documentada
usada mal. Merece una sección porque de ahí sale el requisito más importante del
informe, y porque el fallo fue evitable *desde el producto*.

Los dos «bugs críticos» del reporte original eran «`asigna` del paso N no se
aplica antes de la precondición de N+1» y «`resultado.valor_medido` no funciona en
precondiciones tras pasos WASM». Ninguno existe: `asigna` → Locals funciona con
ejecutor embebido y con WASM. Lo que pasaba es que `resultado.*` **no está ligado
en el contexto de una precondición** —solo durante el `asigna` del propio paso,
entre `set_resultado` y `limpia_resultado` (`crates/motor/src/lib.rs:579` y `:594`)— y
las precondiciones que fallaban eran conjunciones:

```yaml
precondicion: 'locals.v_real > 4.9 && resultado.valor_medido != nothing'
#               ^ cierto                ^ falso siempre  ⇒  && falso
```

Atribuyeron el `false` al primer conjunto. De ahí adoptaron como *workaround* el
propio patrón roto, lo propagaron a 19 secuencias y 51 precondiciones, y luego
reportaron el workaround fallando como un segundo bug crítico. El resto son
variaciones: «no se pueden comparar strings» (se pueden:
`file_globals.lote == "A-2026"` funciona), «el error de `>` con nulo debería ser un
salto» (es fail-fast, y correcto).

**Tres requisitos concretos:**

1. **`resultado.*` fuera del contexto de `asigna` debería ser error de carga, no
   `nothing` silencioso.** Hoy `precondicion: 'resultado.valor_medido != nothing'`
   es YAML válido que se evalúa siempre a `false`, salta el paso, y deja la
   secuencia en verde porque `saltado` no degrada el agregado. Tres capas de
   silencio encadenadas. El cargador ya sabe qué campo está parseando —parsea
   `precondicion` y `asigna` por separado en `a_definicion`—, así que rechazar
   `Scope::Resultado` en `precondicion` es barato y encaja con el fail-fast del
   resto.

2. **La cascada de `saltado` necesita visibilidad.** Una secuencia que salta 9 de
   sus 21 pasos reporta `paso`. Es correcto por diseño, pero el reporte debería
   decir cuántos pasos se saltaron, y convendría un `--strict` que trate un salto
   inesperado como fallo. En la entrega original 9 secuencias daban verde
   saltándose ≥30% de sus pasos.

3. **La documentación de usuario necesita una frase explícita.**
   `docs/planes/m4-nucleo.md:165` deja claro para quien lee el plano de
   implementación que el entorno hace `set_resultado`/`limpia_resultado` alrededor
   del `asigna`. `docs/diseno/variables-y-alcances.md`, que es lo que lee un
   usuario, no lo dice. Una línea —«`resultado.*` solo es visible dentro del
   `asigna` del propio paso; no está disponible en `precondicion`»— habría evitado
   todo esto.

---

## 6. Lo que salió bien

No todo es deuda, y conviene registrarlo:

- **Estabilidad.** Más de 600 ejecuciones, 0 cuelgues, 0 crashes, 0 resultados no
  deterministas. Repetir una secuencia da byte a byte el mismo reporte.
- **Fail-fast del cargador.** Los errores de schema y validación llegan antes de
  ejecutar nada y nombran el paso. Los betatesters lo señalan como lo mejor del
  producto, y el rechazo de `min > max` lo clasificaron ellos mismos como «no es
  bug, es validación correcta».
- **El motor de expresiones aguanta.** Statements con 15+ condiciones encadenadas,
  comparaciones encadenadas estilo Julia, cortocircuito, `nothing`: sin un solo
  fallo atribuible al evaluador.
- **Parameters by-reference con anidamiento profundo.** Verificado a 3 niveles
  (padre → sub externa → sub externa): el valor se propaga correctamente hasta la
  raíz.
- **Coste de arranque medido y lineal.** ~650 ms de arranque en vacío y ~300-400 ms
  por puente WASM, con coste por paso despreciable. Reproducido: 1 WASM ≈ 1,05 s,
  3 WASM ≈ 2,6 s, 6 WASM ≈ 3,6 s. Es el dato que justifica un modo daemon si se
  quiere correr campañas de miles de secuencias.
