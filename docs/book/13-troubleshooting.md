# 13. When something goes wrong

## The executor is not running

The most common mistake: you run a sequence and forgot to start the executor
in the other terminal. In 0.5.0 Anvil does not say so clearly. It retries the
connection for several seconds, printing hundreds of lines of `motor conectado`
and `conexión cerrada; esperando otra` ("engine connected", "connection
closed; waiting for another"), and then ends with:

```console
$ anvil sequences/first.yseq 2>&1 | tail -n 2
conexión cerrada; esperando otra
no se pudo conectar a los ejecutores de pasos (embebido en 127.0.0.1:33973): WASI socket error: ErrorCode { code: 14, name: "connection-refused", message: "The TCP connection was forcefully rejected" }
```

"Could not connect to the step executors … connection refused". Two things in
that message are misleading. It names `embebido`, the engine's built-in
executor, and a port that is not the one you declared — but the executor that
refused is yours, `bench` on 9201. When you see `connection-refused`, check
first that every `grpc` executor in the sequence is running:

```console
$ ss -ltn | grep 9201
```

prints a line when something is listening on that port, and nothing when not.

## An old executor is still running

The opposite problem is worse, because nothing fails. If an executor from an
earlier session is still listening on the port — started in a terminal you
closed, or left behind by a crash — Anvil talks to it, and it answers with
whatever code it was built from. You change a step, run the sequence, and see
the old behaviour.

Before chasing a result that makes no sense, see who owns the port:

```console
$ ss -ltnp | grep 9201
```

The process name and number are at the end of the line; stop it and start your
executor again.

If you start a C# executor while another one holds the port, it prints its
usual `anvil C# executor: … on 127.0.0.1:9201` line and then stops with
`Unhandled exception. System.IO.IOException: Failed to bind to address
http://127.0.0.1:9201: address already in use.` and a stack trace. Believe the
exception, not the first line.

## I changed a step and nothing changed

The executor serves the code it was started with. Stop it and start it again;
`dotnet run` rebuilds. The same goes for the Python executor. Rust modules are
read when Anvil starts the executor, so rebuild and copy the `.wasm` again.

## The sequence does not match the executors

`la secuencia no casa con lo que ofrecen los ejecutores` is Anvil refusing to
run because the sequence asks for something an executor does not offer. The
lines below it are in English and say exactly what: a step the executor does
not serve (chapter 4), an input it does not take or a required one missing
(chapter 7), an output it does not return (chapter 7). Nothing ran.

## Messages that name Spanish keys

Some load errors in 0.5.0 still name the Spanish spelling of a key — for
instance, a subsequence with its own `executors:` section is told to refer to
executors "by name with `ejecutor:`". The key is `executor:`. The sequence
format itself is English only ([#59](https://github.com/anlaco/anvil/issues/59)).

## The AppImage does not start

`dlopen(): error loading libfuse.so.2` means FUSE 2 is not installed; see the
end of chapter 2.
