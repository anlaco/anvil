# 2. Installing

You will end this chapter with one folder holding three things: the Anvil
engine, a copy of the Anvil repository (for the C# step SDK), and an empty C#
project where your steps will go.

```console
$ mkdir ~/anvil-book
$ cd ~/anvil-book
```

## The engine

Anvil is one statically linked binary; there is nothing to install system-wide.
Download it from the [release page](https://github.com/anlaco/anvil/releases/tag/v0.5.0),
check it, and unpack it:

```console
$ curl -sSLO https://github.com/anlaco/anvil/releases/download/v0.5.0/anvil-v0.5.0-x86_64-linux-musl.tar.gz
$ curl -sSLO https://github.com/anlaco/anvil/releases/download/v0.5.0/SHA256SUMS
$ sha256sum --check --ignore-missing SHA256SUMS
anvil-v0.5.0-x86_64-linux-musl.tar.gz: OK
$ tar xzf anvil-v0.5.0-x86_64-linux-musl.tar.gz
```

(`sha256sum` prints `OK` in your system's language.)

The folder it creates holds `anvil`, the engine, and `anvil-exec-wasm`, which
chapter 12 uses. Put it on your `PATH` — in every new terminal you open for
this book:

```console
$ export PATH="$PWD/anvil-v0.5.0-x86_64-linux-musl:$PATH"
```

and check:

```console
$ anvil --version
anvil 0.5.0
```

`anvil --version` writes to the error stream, not to standard output, so
`anvil --version | something` reads nothing
([#75](https://github.com/anlaco/anvil/issues/75)).

**Windows.** The same release has `anvil-v0.5.0-x86_64-windows.zip`, with
`anvil.exe` and `anvil-exec-wasm.exe`. **Not verified** in this book.

## The step SDK

Your steps will use a small library, `Anvil.Step`. In 0.5.0 it is not yet
published on NuGet, so you take it from the repository, at the tag that matches
your engine:

```console
$ git clone --depth 1 --branch v0.5.0 https://github.com/anlaco/anvil.git anvil
```

git will print a note about a "detached HEAD": that is what checking out a tag
looks like, and it is fine. You will not build Anvil from this copy — you only point
your project at the SDK inside it. When the package reaches NuGet, this step
goes away.

## The project

```console
$ dotnet --version
$ dotnet new console --output Bench
```

The first command must print a version starting with `10.`. The second creates
`Bench/`, the executor you will fill with steps from chapter 3 on.

Your folder now looks like this:

```
~/anvil-book/
├── anvil/                               the repository, for the SDK
├── anvil-v0.5.0-x86_64-linux-musl/      the engine
└── Bench/                               your steps
```

## The Sequence Editor

The release also carries the Sequence Editor, a graphical editor for the same
files, as `anvil-editor-v0.5.0-x86_64-linux.AppImage`, a `.deb` and a Windows
installer. This book does not use it yet (see the [contents](README.md)).

If you try the AppImage and it stops at `dlopen(): error loading libfuse.so.2`,
your system lacks FUSE 2, which recent Ubuntu releases no longer install. The
AppImage runs without it once extracted:

```console
$ chmod +x anvil-editor-v0.5.0-x86_64-linux.AppImage
$ ./anvil-editor-v0.5.0-x86_64-linux.AppImage --appimage-extract
$ ./squashfs-root/AppRun
```
