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

On Windows, follow [the Windows section](#on-windows) at the end of this
chapter instead.

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

## On Windows

Everything above has a Windows equivalent, in **Windows PowerShell** — the one
Windows already has; open it from the Start menu. This section was run on a
GitHub Actions Windows machine (Windows Server 2025, build 26100, Windows
PowerShell 5.1), with the downloads marked as coming from the Internet as a
browser marks them. It has **not** been run on a Windows desktop: what
Windows' own security prompts do there is **not verified**.

### Download

On the [release page](https://github.com/anlaco/anvil/releases/tag/v0.5.0),
under **Assets**, download two files. Your browser saves them in `Downloads`:

- `anvil-v0.5.0-x86_64-windows.zip` — the engine;
- `SHA256SUMS` — the checksums, to confirm the download is intact.

### Check and unpack

```console
PS> mkdir $HOME\anvil-book
PS> cd $HOME\anvil-book
PS> Move-Item "$HOME\Downloads\anvil-v0.5.0-x86_64-windows.zip", "$HOME\Downloads\SHA256SUMS" .
PS> (Get-FileHash .\anvil-v0.5.0-x86_64-windows.zip -Algorithm SHA256).Hash
2F2AA5AF9044B2BBBA42E66434361EBC220EF1849E73F927FFC50121307EC96A
PS> Select-String anvil-v0.5.0-x86_64-windows.zip .\SHA256SUMS
C:\Users\you\anvil-book\SHA256SUMS:5:2f2aa5af9044b2bbba42e66434361ebc220ef1849e73f927ffc50121307ec96a  anvil-v0.5.0-x86_64-windows.zip
```

The two hashes must be the same digits; PowerShell prints them in capitals and
the file in lower case. Then unpack:

```console
PS> Expand-Archive .\anvil-v0.5.0-x86_64-windows.zip -DestinationPath .
```

It creates `anvil-v0.5.0-x86_64-windows\`, with `anvil.exe` and
`anvil-exec-wasm.exe`. They are built not to need the Visual C++ runtime; the
test machine had it installed anyway, so a machine without it is **not
verified**.

If you unpack with the Explorer instead (right click → *Extract All*), the files
inside may keep the "downloaded from the Internet" mark. On the test machine
`anvil.exe` ran from PowerShell with the mark on; if your Windows refuses to run
it, remove the mark from the whole folder:

```console
PS> Get-ChildItem -Recurse .\anvil-v0.5.0-x86_64-windows | Unblock-File
```

Neither `anvil.exe` nor the editor installer is digitally signed in 0.5.0, so
Windows may warn before running them. That warning was not reproduced here.

### Put it on the path

In every PowerShell window you open for this book:

```console
PS> $env:Path = "$PWD\anvil-v0.5.0-x86_64-windows;$env:Path"
PS> anvil --version
anvil 0.5.0
```

### The SDK and the project

The same commands as on Linux, and git and the .NET 10 SDK must be installed
first:

```console
PS> git clone --depth 1 --branch v0.5.0 https://github.com/anlaco/anvil.git anvil
PS> dotnet --version
PS> dotnet new console --output Bench
```

```
C:\Users\you\anvil-book\
├── anvil\                               the repository, for the SDK
├── anvil-v0.5.0-x86_64-windows\         the engine
└── Bench\                               your steps
```

### The rest of the book on Windows

The chapters are written for a Linux shell. Chapters 3 and 4 were also run on
the Windows machine, up to running `sequences/first.yseq` and reading its JSON
report: the C# project, `dotnet run` and `anvil` behave the same and print the
same output. Paths work with either `/` or `\`. Where a command in the book is
shell syntax, use:

| in the book | in Windows PowerShell |
|---|---|
| `~/anvil-book` | `$HOME\anvil-book` |
| `export PATH="$PWD/anvil-v0.5.0-x86_64-linux-musl:$PATH"` | `$env:Path = "$PWD\anvil-v0.5.0-x86_64-windows;$env:Path"` |
| `2>/dev/null` | `2>$null` |
| `echo $?` | `$LASTEXITCODE` |
| `cat data.json` | `Get-Content data.json` |
| `ss -ltn \| grep 9201` | `Get-NetTCPConnection -LocalPort 9201 -State Listen` |

A few sessions pipe into `tail` or `grep` only to shorten what they show; on
Windows, run the command without them. Chapters 5 to 13 and the Python and Rust
steps of chapter 12 have **not** been run on Windows.

### The Sequence Editor on Windows

Download `anvil-editor-v0.5.0-x86_64-windows-setup.exe` from the same page and
run it. On the test machine it installed for the current user, into
`%LOCALAPPDATA%\Programs\@anvileditor\`, and added **Anvil Sequence Editor** to
the Start menu. That was checked with a silent install (`setup.exe /S`); the
installer's own window, and whether it asks for administrator rights, were not.
Remember that in 0.5.0 the packaged editor cannot run a sequence
([#80](https://github.com/anlaco/anvil/issues/80)).
