# La carpeta de ejecutores instalados

*ADR-0046 §4. Este documento describe el formato del manifiesto y dónde se
instala; el porqué está en la ADR.*

Una secuencia dice **dónde escucha** un ejecutor y nada más. Su bloque
opcional `dev:` dice cómo levantarlo en una máquina de desarrollo, y ahí el
`runtime` es un **nombre lógico** — `wasm`, `python` —, nunca una ruta: una
ruta sería lo único del archivo que cambia de una máquina a otra, en un archivo
que todo el mundo commitea.

Ese nombre se resuelve aquí:

```
~/.anvil/executors/
  wasm/     executor.json  anvil-exec-wasm
  python/   executor.json  anvil-exec-python  server.py  anvil_step/ …
```

`ANVIL_HOME` mueve la carpeta entera (`$ANVIL_HOME/executors`). Es una
variable y no una lista de sitios donde buscar: varios sitios es justamente
cómo un nombre empieza a significar código distinto en dos máquinas, que es el
fallo silencioso por el que la ADR-0046 descartó un archivo de estación.

## `executor.json`

```json
{
  "runtime": "wasm",
  "exec": "anvil-exec-wasm",
  "code": "--modules",
  "port": "--port",
  "eof": "--exit-on-eof"
}
```

| campo     | qué es                                                        |
| --------- | ------------------------------------------------------------- |
| `runtime` | el nombre lógico. Tiene que coincidir con el de la carpeta.    |
| `exec`    | el ejecutable, dentro de esta misma carpeta.                   |
| `code`    | **el flag que recibe el código**: `--modules`, `--steps`, …    |
| `port`    | el flag que recibe el puerto.                                  |
| `eof`     | opcional: el flag con el que se muere al cerrarse su stdin.    |

`code` es la única indirección que la carpeta de instalación tiene que
normalizar, y es la que permite que una herramienta arranque un runtime del que
no sabe nada. Todos los ejecutores reciben su código por un flag y su dirección
por otro; lo que no hacen es llamarlos igual.

`eof` es lo único del manifiesto que habla de *quien lo lanza* y no del
ejecutor. Una herramienta que arranca uno lo para al cerrarse, pero eso cubre
los cierres ordenados y nada más: si la matan o se cae, no corre código, y un
ejecutor huérfano se queda con el puerto que declara la siguiente secuencia sin
que nada en pantalla diga que está ahí. Con `eof`, quien lo lanza le da un
stdin que nunca escribe; la tubería se cierra sola cuando el padre muere, como
muera, y el ejecutor se va con él.

Un runtime sin `eof` no está mal declarado: se recoge en los cierres ordenados
y no en una caída, y conviene saberlo en vez de suponerlo. `anvil-exec-python`
todavía no tiene un flag así.

Que `runtime` repita el nombre de la carpeta no es redundante: una carpeta
copiada con otro nombre y dejada diciendo el anterior es un runtime que
responde a dos nombres, y se rechaza al leerla.

Que `eof` sea opcional y `code` no lo sea no es casual: sin `code` no se puede
arrancar nada, y sin `eof` se arranca igual.

En Windows el nombre de `exec` no es el archivo — `anvil-exec-wasm` es
`anvil-exec-wasm.exe` —, así que quien lee el manifiesto prueba también `.exe`,
`.cmd` y `.bat`, y los nombra si no encuentra ninguno.

## Instalar

Desde el árbol de fuentes:

```sh
make install-executors            # a ~/.anvil/executors
ANVIL_HOME=/opt/anvil make install-executors
```

Desde el paquete descargado, la carpeta `executors/` que trae dentro se copia
tal cual:

```sh
mkdir -p ~/.anvil && cp -r executors ~/.anvil/
```

## Quién lo lee

Sólo herramientas. El editor de secuencias, para poder levantar un ejecutor
mientras alguien escribe la secuencia y así tener a quién preguntarle su
catálogo (ADR-0044). **El motor no lee `dev:` ni esta carpeta**: una secuencia
se comporta igual en un banco esté el bloque o no.

## Lo que todavía no está decidido

- Cómo se versiona un runtime contra el contrato de paso.
- Si una configuración de máquina puede añadir sitios donde buscar.
- Si `dev:` puede nombrar un runtime que la carpeta no tiene, para un banco que
  trae el suyo.
- `anvil-exec-python` es un script con shebang: en Windows todavía no hay una
  forma acordada de nombrarlo en `exec` (haría falta el intérprete, y el
  manifiesto no tiene campo para eso).
