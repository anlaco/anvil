# Ejecutores de lenguaje

Un servidor gRPC por lenguaje que habla el contrato
[`paso.proto`](../crates/modelo/paso.proto). El motor de Anvil los ve como
endpoints: despacha por nombre→endpoint y no sabe qué hay detrás
([ADR-0012](../docs/adr/0012-executores-de-lenguaje-como-modulos.md)).

| | |
|---|---|
| [`python/`](python/) | El primero. Escribes un paso como función y lo dejas en una carpeta. |
| LabVIEW, MATLAB, … | Futuros. Cada uno en su subdirectorio, con la misma forma. |

Son **alternativas, no capas**: eliges el que necesites, puedes arrancar
varios a la vez, y mezclarlos en la misma secuencia. El ejecutor WASM que
Anvil trae de serie (`crates/ejecutor_pasos`) no vive aquí: es parte del
núcleo y va embebido en el binario.

## Licencia: **Apache-2.0**, y no la del resto del repo

> Todo lo que cuelga de este directorio es **Apache-2.0** ([`LICENSE`](LICENSE)),
> no AGPL-3.0. Anvil —el secuenciador, la raíz del repo— sí es AGPL.

La frontera no es caprichosa y está en
[ADR-0004](../docs/adr/0004-licencia-dual-agpl-apache.md): **lo que se *usa*
es AGPL; lo que se *linka* es Apache.**

Anvil se usa: le pasas una secuencia y te devuelve un veredicto. Un ejecutor
de lenguaje no: su SDK entra **dentro de tu código** en cuanto escribes
`from anvil_step import step`. Copyleft ahí sería copyleft sobre tus pasos de
test, que es exactamente lo que ADR-0004 decide evitar — igual que con
`wasi-grpc` y `wasi-visa`.

**Tus pasos y tus secuencias son tuyos**, con la licencia que quieras, y no se
contagian de nada. Los límites de aceptación y el know-how de producto que hay
en una secuencia siguen siendo tuyos.

Los ficheros llevan además su `SPDX-License-Identifier` en cabecera: este
directorio está dentro de un repositorio AGPL, y un fichero suelto que alguien
copie fuera tiene que seguir diciendo bajo qué licencia va.
