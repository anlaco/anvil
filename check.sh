#!/bin/bash
# Runs every command the book shows and compares what comes out with what the
# book says comes out.
#
# The book is only worth anything while it is true, and it stops being true
# the first time the engine changes a message. So nothing in a chapter is
# typed by hand: every terminal session is a file under listings/sessions/,
# every file the reader writes is a file under listings/, and this script
# checks both — that each session still produces its output, and that each
# of those files still appears word for word in some chapter.
#
# Usage (from anywhere):
#   docs/book/check.sh <dir with the anvil binary>      # compare
#   docs/book/check.sh --record <dir>                   # rewrite the sessions
#
# The directory is an unpacked release, e.g. anvil-v0.5.0-x86_64-linux-musl/.
# The C# SDK is taken from this repository, as the reader takes it from their
# clone. Needs dotnet. The Python sessions also need PYTHON pointing at an
# interpreter with grpcio and grpcio-tools; the Rust ones need cargo with the
# wasm32-wasip2 target. Without them those sessions say SKIP, never OK.
#
# --record is for writing a new session, not for making a red one green:
# read the diff of what it rewrote before committing it.

set -u

RECORD=0
if [ "${1:-}" = "--record" ]; then RECORD=1; shift; fi
DIST=${1:-}
if [ -z "$DIST" ] || [ ! -x "$DIST/anvil" ]; then
  echo "usage: $0 [--record] <dir holding the anvil binary>" >&2
  exit 2
fi
DIST=$(cd "$DIST" && pwd)

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BOOK=$ROOT/docs/book
L=$BOOK/listings

# `anvil --version` writes to stderr (#75).
VERSION=$("$DIST/anvil" --version 2>&1 | awk '{print $2}')
DISTNAME=anvil-v$VERSION-x86_64-linux-musl

for port in 9101 9201; do
  if ss -ltn | grep -q ":$port "; then
    echo "port $port is already taken. A stale executor answers plausibly and wrongly;" >&2
    echo "find it with: ss -ltnp | grep :$port" >&2
    exit 2
  fi
done

WORK=$(mktemp -d)
PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null; done
  rm -rf "$WORK"
}
trap cleanup EXIT

# The reader's folder, as chapter 2 has them build it. The clone is a copy of
# only what the book uses, without build output: a symlink back into this tree
# would share its obj/ directories, and MSBuild does not survive that.
mkdir -p "$WORK/anvil/crates/modelo"
cp "$ROOT/crates/modelo/paso.proto" "$WORK/anvil/crates/modelo/"
rsync -a --exclude bin --exclude obj --exclude target --exclude __pycache__ \
  --exclude 'paso_pb2*.py' "$ROOT/executors" "$WORK/anvil/"
ln -s "$DIST" "$WORK/$DISTNAME"
cp -r "$L/sequences" "$L/python_steps" "$L/board-wasm" "$WORK/"
mkdir -p "$WORK/Bench"
cp "$L/Bench/Bench.csproj" "$L/Bench/Program.cs" "$WORK/Bench/"
export PATH="$WORK/$DISTNAME:$PATH"

ok=0; fail=0; skip=0
say() { # say <OK|FAIL|SKIP> <what>
  case $1 in
    OK)   printf '  \033[32mOK  \033[0m %s\n' "$2"; ok=$((ok + 1)) ;;
    FAIL) printf '  \033[31mFAIL\033[0m %s\n' "$2"; fail=$((fail + 1)) ;;
    SKIP) printf '  \033[33mSKIP\033[0m %s\n' "$2"; skip=$((skip + 1)) ;;
  esac
}

# What changes from one run to the next and says nothing about the book.
normalise() {
  sed -E \
    -e 's/life [0-9a-f]{32}/life <life>/g' \
    -e 's/"lifetime": "[0-9a-f]{32}"/"lifetime": "<life>"/g' \
    -e 's/sha256:[0-9a-f]+/sha256:<sha256>/g' \
    -e 's/escuchando en [0-9]+/escuchando en <port>/g' \
    -e 's/127\.0\.0\.1:[0-9]{5}/127.0.0.1:<port>/g'
}

# Which chapter introduces each step file: a session of chapter N runs against
# the executor as the reader has it at chapter N, not as it ends up.
stage_of() {
  case $1 in
    Board.cs) echo 03 ;;
    Board.Judging.cs) echo 05 ;;
    Fixture.cs) echo 06 ;;
    Dmm.cs | Unit.cs) echo 07 ;;
    PowerSupply.cs) echo 08 ;;
    *) echo 99 ;;
  esac
}

CS_PID=""
CS_STAGE=""
csharp_at() { # csharp_at <chapter>
  local want="" f
  for f in "$L"/Bench/*.cs; do
    f=$(basename "$f")
    [ "$f" = Program.cs ] && continue
    [ "$(stage_of "$f")" -le "$1" ] && want="$want $f"
  done
  [ "$want" = "$CS_STAGE" ] && return 0
  [ -n "$CS_PID" ] && { kill "$CS_PID"; wait "$CS_PID" 2>/dev/null; }
  find "$WORK/Bench" -maxdepth 1 -name '*.cs' ! -name Program.cs -delete
  for f in $want; do cp "$L/Bench/$f" "$WORK/Bench/"; done
  if ! (cd "$WORK/Bench" && dotnet build -nologo -v q >"$WORK/build.log" 2>&1); then
    cat "$WORK/build.log" >&2
    echo "the C# listings do not build at chapter $1" >&2
    exit 1
  fi
  (cd "$WORK/Bench" && exec dotnet run --no-build -- --port 9201 >"$WORK/csharp.log" 2>&1) &
  CS_PID=$!
  PIDS+=("$CS_PID")
  for _ in $(seq 60); do ss -ltn | grep -q ':9201 ' && break; sleep 0.5; done
  CS_STAGE=$want
}

PY_PID=""
python_up() {
  [ -n "$PY_PID" ] && return 0
  [ -n "${PYTHON:-}" ] && "$PYTHON" -c 'import grpc, grpc_tools' 2>/dev/null || return 1
  local py=$WORK/anvil/executors/python
  if [ ! -f "$py/paso_pb2.py" ]; then
    (cd "$py" && "$PYTHON" -m grpc_tools.protoc -I ../../crates/modelo \
      --python_out=. --grpc_python_out=. ../../crates/modelo/paso.proto) || return 1
  fi
  (cd "$WORK" && exec "$PYTHON" anvil/executors/python/anvil-exec-python --steps python_steps \
    >"$WORK/python.log" 2>&1) &
  PY_PID=$!
  PIDS+=("$PY_PID")
  for _ in $(seq 40); do ss -ltn | grep -q ':9101 ' && break; sleep 0.5; done
}

wasm_up() {
  [ -x "$WORK/wasm-dept/anvil-exec-wasm" ] && return 0
  command -v cargo >/dev/null || return 1
  (cd "$WORK" && cargo build -q --target wasm32-wasip2 --manifest-path board-wasm/Cargo.toml \
    >"$WORK/cargo.log" 2>&1) || return 1
  mkdir -p "$WORK/wasm-dept"
  cp "$WORK/$DISTNAME/anvil-exec-wasm" "$WORK/board-wasm/target/wasm32-wasip2/debug/board.wasm" \
    "$WORK/wasm-dept/"
}

# A session file is what a terminal shows: `$ command` lines, each followed by
# its output. Running one means running its commands in order in one shell, in
# the reader's folder, and printing the same transcript.
run_session() { # run_session <file>
  local script=$WORK/session.sh cmd
  : >"$script"
  while IFS= read -r line; do
    case $line in
      '$ '*)
        cmd=${line#'$ '}
        # Echoing the prompt must not clobber $?, or `echo $?` would lie.
        printf '__st=$?; printf "%%s\\n" %q; (exit $__st)\n' "$line" >>"$script"
        case $cmd in
          # The executor is already up; what the reader sees is its first line.
          'dotnet run --project Bench -- --port 9201') echo "head -n 1 '$WORK/csharp.log'" >>"$script" ;;
          '.venv/bin/python anvil/executors/python/anvil-exec-python --steps python_steps')
            echo "head -n 2 '$WORK/python.log'" >>"$script" ;;
          *) printf '{ %s ; } 2>&1\n' "$cmd" >>"$script" ;;
        esac
        ;;
    esac
  done <"$1"
  (cd "$WORK" && bash "$script") | sed "s#$WORK#/home/you/anvil-book#g"
}

echo "anvil $VERSION from $DIST"
echo "sessions:"
for session in "$L"/sessions/*.txt; do
  name=$(basename "$session" .txt)
  chapter=${name%%-*}
  case $name in
    *python*) python_up || { say SKIP "$name (PYTHON with grpcio and grpcio-tools not given)"; continue; } ;;
    *wasm*) wasm_up || { say SKIP "$name (cargo with wasm32-wasip2 not available)"; continue; } ;;
  esac
  case $chapter in
    0[3-9] | 1[0-2]) csharp_at "$chapter" ;;
    # Troubleshooting starts from the executor the reader forgot to start.
    13) [ -n "$CS_PID" ] && { kill "$CS_PID"; wait "$CS_PID" 2>/dev/null; CS_PID=""; CS_STAGE=""; } ;;
  esac
  actual=$(run_session "$session")
  if [ "$RECORD" = 1 ]; then
    printf '%s\n' "$actual" >"$session"
    say OK "$name (recorded)"
  elif diff <(printf '%s\n' "$actual" | normalise) <(normalise <"$session") >"$WORK/diff"; then
    say OK "$name"
  else
    say FAIL "$name"
    sed 's/^/      /' "$WORK/diff"
  fi
done

echo "listings quoted verbatim in a chapter:"
while IFS= read -r line; do
  case $line in
    OK*) say OK "${line#OK }" ;;
    *) say FAIL "${line#MISSING } is not quoted word for word in any chapter" ;;
  esac
done < <(python3 - "$BOOK" <<'EOF'
import pathlib, sys
book = pathlib.Path(sys.argv[1])
chapters = "\n".join(p.read_text() for p in sorted(book.glob("[0-9][0-9]-*.md")))
listings = book / "listings"
for path in sorted(listings.rglob("*")):
    if not path.is_file() or "target" in path.parts:
        continue
    text = path.read_text().rstrip("\n")
    rel = path.relative_to(listings)
    print(("OK " if text in chapters else "MISSING ") + str(rel))
EOF
)

echo "$ok OK, $fail FAIL, $skip SKIP"
[ "$fail" -eq 0 ]
