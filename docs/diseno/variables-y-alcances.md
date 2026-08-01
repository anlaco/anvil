# Diseño: Variables y alcances

> **Prioridad:** MVP-parcial. **Propuesta** (no implementado). Locals /
> Parameters / FileGlobals en MVP-parcial; StationGlobals post-MVP.

Modelo mental de TestStand: "una hoja de cálculo para tests" — variables
tipadas con alcance, para cablear datos entre pasos sin código pegamento
([investigación](../investigacion/TestStand-y-competencia.md) §1.4). Anvil
adopta la jerarquía **sin** replicar el árbol de propiedades opaco de
TestStand.

## Scopes (propuesta)

| Scope | Visible en | Cuándo se crea | MVP |
|---|---|---|---|
| **Locals** | Una secuencia | Al iniciar su ejecución | MVP-parcial |
| **Parameters** | Secuencia llamada (entrada/salida) | Al invocar via sequence call | MVP-parcial |
| **FileGlobals** | Todas las secuencias de un archivo | Al cargar el archivo | MVP-parcial |
| **StationGlobals** | Todas las secuencias de la estación | Persistente en la estación | post-MVP |

## En el formato de secuencia (YAML)

```yaml
nombre: basica
locals:
  voltaje_leido: 0.0
parameters: {}            # al llamar desde otra secuencia
file_globals:
  lote: "A-2026-08"

setup:
  - nombre: conectar_equipo
    reintentos: 3
main:
  - nombre: medir_voltaje
    reintentos: 1
    asigna: { voltaje_leido: "${resultado.valor_medido}" }
```

## Reglas de acceso

- **Lectura:** un paso puede leer variables de su scope y de los superiores
  (Locals < Parameters < FileGlobals < StationGlobals).
- **Escritura:** un paso escribe su resultado en la variable indicada
  (`asigna`), y muta solo **Locals** de su secuencia (no FileGlobals ni
  StationGlobals — eso lo hace el motor, no el paso, para mantener el paso
  aislado por contrato).
- **Tipado:** declaración con tipo básico (numérico, texto, booleano); la
  validación es al cargar (fail-fast). Sin el árbol de propiedades tipado
  recursivo de TestStand en el MVP.

## Por qué este recorte

El motor es genérico (ADR-0005): no conoce el dominio. Las variables son
**datos** en la secuencia que el paso recibe/produce vía el contrato. El
paso no lee variables "del motor" directamente: el motor **inyecta** los
valores relevantes en la petición (post-MVP, cuando el contrato lleve
parámetros tipados — ver [contrato-grpc.md](../contrato-grpc.md)) y recoge
el resultado. Así se preserva el aislamiento.

## StationGlobals (post-MVP)

Persistencia por estación (configuración de la línea, calibración). Requiere
un almacén local y un modelo de concurrencia (escritura segura entre
secuencias paralelas). Por eso es post-MVP.

## Out-of-scope

- Árbol de propiedades recursivo tipado de TestStand (complejo, opaco).
- Referencias cruzadas con expresiones complejas en el MVP.