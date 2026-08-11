#!/usr/bin/env python3
"""
Betatester automático para Anvil.

Flujo:
1. Genera secuencias YAML aleatorias (ejercitando features del producto).
2. Las corre contra el binario `anvil` compilado.
3. Registra resultados (paso/fallo/error/crash/timeout).
4. Si --validate falla, es un bug del cargador.
5. Si la ejecución crashea o da error inesperado, es un bug del motor.
6. Guarda un log estructurado en betatester/logs/.

Uso:
    python3 betatester/run.py [--anvil /path/to/anvil] [--n 5] [--timeout 30]

El script está pensado para correr cada hora vía cron.
"""

import subprocess
import json
import sys
import os
import time
import tempfile
import shutil
import random
from pathlib import Path
from datetime import datetime

# Resolver paths relativos al repo
SCRIPT_DIR = Path(__file__).parent.resolve()
REPO_DIR = SCRIPT_DIR.parent
GENERADOR = SCRIPT_DIR / "generador.py"
LOGS_DIR = SCRIPT_DIR / "logs"
WORK_DIR = SCRIPT_DIR / "work"

# Buscar el binario anvil
ANVIL_CANDIDATES = [
    REPO_DIR / "packaging/anvil-host/target/release/anvil",
    REPO_DIR / "packaging/anvil-host/target/debug/anvil",
]


def find_anvil():
    for p in ANVIL_CANDIDATES:
        if p.is_file() and os.access(p, os.X_OK):
            return str(p)
    return None


def run_cmd(cmd, timeout=30, cwd=None):
    """Ejecuta un comando y devuelve (returncode, stdout, stderr)."""
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=cwd or str(REPO_DIR),
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return -1, "", "TIMEOUT"
    except Exception as e:
        return -2, "", str(e)


def run_sequence(anvil_path, yaml_path, flags_extra=None, sidecar=None,
                 timeout=30):
    """Corre una secuencia contra anvil y devuelve el resultado estructurado."""
    flags_extra = flags_extra or []
    cmd = [anvil_path]

    # Convertir a path relativo al repo (anvil usa CWD para resolver)
    rel_yaml = os.path.relpath(str(yaml_path), str(REPO_DIR))

    # Si --validate está en los flags, sólo validar
    if "--validate" in flags_extra:
        cmd.extend(["--validate", rel_yaml])
        rc, out, err = run_cmd(cmd, timeout=timeout)
        return {
            "modo": "validate",
            "cmd": " ".join(cmd),
            "rc": rc,
            "stdout": out,
            "stderr": err,
            "resultado": "valido" if rc == 0 else "invalido",
        }

    # Construir comando de ejecución
    json_out = None
    csv_out = None

    if "--json" in flags_extra:
        json_out = tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False,
            dir=str(WORK_DIR))
        json_out.close()
        rel_json = os.path.relpath(json_out.name, str(REPO_DIR))
        cmd.extend(["--json", rel_json])

    if "--csv" in flags_extra:
        csv_out = tempfile.NamedTemporaryFile(
            mode="w", suffix=".csv", delete=False,
            dir=str(WORK_DIR))
        csv_out.close()
        rel_csv = os.path.relpath(csv_out.name, str(REPO_DIR))
        cmd.extend(["--csv", rel_csv])

    if sidecar:
        rel_sidecar = os.path.relpath(str(sidecar), str(REPO_DIR))
        cmd.extend(["--limits", rel_sidecar])

    cmd.append(rel_yaml)

    rc, out, err = run_cmd(cmd, timeout=timeout)

    result = {
        "modo": "ejecucion",
        "cmd": " ".join(cmd),
        "rc": rc,
        "stdout": out,
        "stderr": err,
        "json_out": None,
        "csv_out": None,
    }

    # Intentar leer el JSON de salida si se pidió
    if json_out and os.path.exists(json_out.name):
        try:
            with open(json_out.name) as f:
                result["json_out"] = json.load(f)
        except Exception:
            result["json_out"] = None
        os.unlink(json_out.name)

    if csv_out and os.path.exists(csv_out.name):
        try:
            with open(csv_out.name) as f:
                result["csv_out"] = f.read()
        except Exception:
            result["csv_out"] = None
        os.unlink(csv_out.name)

    # Clasificar el resultado
    if rc == -1:
        result["resultado"] = "timeout"
    elif rc == -2:
        result["resultado"] = "crash"
    elif rc != 0:
        result["resultado"] = "error_runtime"
    elif ": paso ===" in out:
        result["resultado"] = "paso"
    elif ": fallo ===" in out:
        result["resultado"] = "fallo"
    elif ": error ===" in out:
        result["resultado"] = "error"
    else:
        result["resultado"] = "desconocido"

    return result


def run_one(anvil_path, n=5, timeout=30):
    """Genera y corre una tanda de secuencias."""
    LOGS_DIR.mkdir(parents=True, exist_ok=True)
    WORK_DIR.mkdir(parents=True, exist_ok=True)

    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_path = LOGS_DIR / f"{ts}.json"

    # Generar secuencias (pasar n al generador)
    gen_cmd = ["python3", str(GENERADOR), str(WORK_DIR), str(n)]
    rc, out, err = run_cmd(gen_cmd, timeout=10)
    if rc != 0:
        print(f"Error generando secuencias: {err}", file=sys.stderr)
        return None

    try:
        gen_info = json.loads(out)
    except Exception:
        gen_info = {"raw": out}

    secuencias = gen_info.get("secuencias", [])
    if not secuencias:
        print("No se generaron secuencias", file=sys.stderr)
        return None

    resultados = []
    for yaml_path in secuencias:
        yaml_path = Path(yaml_path)

        # Determinar sidecar
        sidecar = None
        sidecar_candidate = yaml_path.with_suffix(".limits.yaml")
        if sidecar_candidate.exists():
            sidecar = str(sidecar_candidate)

        # Flags extra
        flags = []
        if random.random() < 0.7:
            flags.append("--json")
        if random.random() < 0.3:
            flags.append("--csv")
        if random.random() < 0.15:
            flags.append("--validate")

        # Flags para ejecución (sin --validate, que se hace aparte)
        exec_flags = [f for f in flags if f != "--validate"]

        # Ejecutar la secuencia
        result = run_sequence(
            anvil_path, yaml_path,
            flags_extra=exec_flags,
            sidecar=sidecar,
            timeout=timeout,
        )

        # También validar siempre (para detectar bugs del cargador)
        val_result = run_sequence(
            anvil_path, yaml_path,
            flags_extra=["--validate"],
            sidecar=sidecar,
            timeout=10,
        )

        entrada = {
            "timestamp": datetime.now().isoformat(),
            "yaml": str(yaml_path),
            "yaml_name": yaml_path.name,
            "sidecar": sidecar,
            "flags": flags,
            "ejecucion": result,
            "validacion": val_result,
        }
        resultados.append(entrada)

        # Print resumen
        print(f"  {yaml_path.name}: "
              f"valida={val_result['resultado']} "
              f"ejec={result['resultado']}")

    # Compilar resumen
    resumen = {
        "timestamp": ts,
        "anvil": anvil_path,
        "n_secuencias": len(resultados),
        "resultados": resultados,
        "resumen": {
            "validas": sum(1 for r in resultados if r["validacion"]["resultado"] == "valido"),
            "invalidas": sum(1 for r in resultados if r["validacion"]["resultado"] == "invalido"),
            "paso": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "paso"),
            "fallo": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "fallo"),
            "error": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "error"),
            "error_runtime": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "error_runtime"),
            "timeout": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "timeout"),
            "crash": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "crash"),
            "desconocido": sum(1 for r in resultados if r["ejecucion"]["resultado"] == "desconocido"),
        },
    }

    # Guardar log
    with open(log_path, "w") as f:
        json.dump(resumen, f, indent=2, default=str)

    # Limpiar work dir (conservar los YAML por si hay que investigar)
    # shutil.rmtree(WORK_DIR, ignore_errors=True)

    return resumen


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Betatester automático de Anvil")
    parser.add_argument("--anvil", default=None, help="Path al binario anvil")
    parser.add_argument("--n", type=int, default=5, help="Número máximo de secuencias a correr")
    parser.add_argument("--timeout", type=int, default=30, help="Timeout por ejecución (segundos)")
    args = parser.parse_args()

    anvil = args.anvil or find_anvil()
    if not anvil:
        print("No encuentro el binario anvil. Constrúyelo con `make release`",
              file=sys.stderr)
        sys.exit(2)

    print(f"Anvil: {anvil}")
    print(f"Betatester: generando y corriendo hasta {args.n} secuencias...")
    print()

    resumen = run_one(anvil, n=args.n, timeout=args.timeout)
    if resumen:
        s = resumen["resumen"]
        print()
        print(f"== Resumen {resumen['timestamp']} ==")
        print(f"  Secuencias: {resumen['n_secuencias']}")
        print(f"  Válidas: {s['validas']}  Inválidas: {s['invalidas']}")
        print(f"  Paso: {s['paso']}  Fallo: {s['fallo']}  Error: {s['error']}")
        print(f"  Error runtime: {s['error_runtime']}  "
              f"Timeout: {s['timeout']}  Crash: {s['crash']}  "
              f"Desconocido: {s['desconocido']}")
        print(f"  Log: {LOGS_DIR / (resumen['timestamp'] + '.json')}")

        # Alertar si hay algo inesperado
        if s["crash"] > 0 or s["error_runtime"] > 0 or s["timeout"] > 0:
            print()
            print("⚠️  HAY RESULTADOS INESPERADOS — revisa el log")
        if s["invalidas"] > 0 and s["validas"] > 0:
            # Algunas inválidas es normal (edge cases que generan YAML malo)
            pass


if __name__ == "__main__":
    main()