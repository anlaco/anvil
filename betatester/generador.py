#!/usr/bin/env python3
"""
Generador de secuencias YAML aleatorias para betatesting de Anvil.

Crea secuencias válidas que ejercitan las distintas features del secuenciador:
- Pasos gRPC del ejecutor embebido (conectar_equipo, medir_voltaje, verificar_led,
  abrir_rele, desconectar_equipo, identificar_uut, notificar_resultado)
- Límites (rango, comparacion)
- Variables (locals, file_globals, parameters)
- Precondiciones
- disable / pause_on_fail
- Statements (tipo: statement)
- pass_fail (veredicto compuesto)
- sequence_call (subsecuencias inline y externas)
- Ejecutores múltiples (embebido)
- Process model
- Flags: --json, --csv, --validate, --limits

Cada secuencia generada es sintácticamente válida. El betatester la corre
y registra si Anvil la acepta, la ejecuta bien, o falla inesperendidamente.
"""

import random
import os
import sys
import json
import string
import yaml
from pathlib import Path
from datetime import datetime

# Pasos disponibles en el ejecutor embebido
PASOS_EMBEBIDOS = [
    "conectar_equipo",
    "medir_voltaje",
    "verificar_led",
    "abrir_rele",
    "desconectar_equipo",
    "identificar_uut",
    "notificar_resultado",
]

# Pasos que tienen sentido en setup
PASOS_SETUP = ["conectar_equipo", "abrir_rele", "identificar_uut"]
# Pasos que tienen sentido en cleanup
PASOS_CLEANUP = ["desconectar_equipo", "notificar_resultado"]
# Pasos que tienen sentido en main
PASOS_MAIN = ["medir_voltaje", "verificar_led", "abrir_rele", "conectar_equipo"]

# Seed para reproducibilidad
SEED = None  # None = aleatorio cada vez


def rand_name(prefix="paso"):
    """Genera un nombre de paso único con sufijo aleatorio."""
    suffix = "".join(random.choices(string.ascii_lowercase + string.digits, k=4))
    return f"{prefix}_{suffix}"


def gen_limite_rango():
    """Genera un límite de tipo rango con valores aleatorios."""
    min_val = round(random.uniform(3.0, 4.8), 2)
    max_val = round(min_val + random.uniform(0.5, 3.0), 2)
    return {"tipo": "rango", "min": min_val, "max": max_val}


def gen_limite_comparacion():
    """Genera un límite de tipo comparacion."""
    op = random.choice(["ge", "le", "gt", "lt", "eq"])
    esperado = round(random.uniform(3.0, 6.0), 2)
    return {"tipo": "comparacion", "op": op, "esperado": esperado}


def gen_limite():
    """Genera un límite aleatorio (o ninguno)."""
    r = random.random()
    if r < 0.3:
        return None
    elif r < 0.65:
        return gen_limite_rango()
    else:
        return gen_limite_comparacion()


def gen_locals():
    """Genera variables locals aleatorias. Devuelve (dict, tipos)."""
    locals_dict = {}
    locals_types = {}
    n = random.randint(0, 3)
    for _ in range(n):
        name = rand_name("var")
        tipo = random.choice(["bool", "num", "str"])
        if tipo == "bool":
            locals_dict[name] = random.choice([True, False])
            locals_types[name] = "bool"
        elif tipo == "num":
            locals_dict[name] = round(random.uniform(0, 10), 2)
            locals_types[name] = "num"
        else:
            locals_dict[name] = f"lote-{random.randint(1, 99)}"
            locals_types[name] = "str"
    return locals_dict, locals_types


def gen_file_globals():
    """Genera file_globals aleatorios."""
    fg = {}
    n = random.randint(0, 2)
    for _ in range(n):
        name = rand_name("fg")
        fg[name] = f"batch-{random.randint(100, 999)}"
    return fg


def gen_paso_main(usar_reintentos=True, usar_limite=True, usar_precondicion=True,
                  usar_disable=True, usar_pause=True, usar_statements=True,
                  locals_refs=None, locals_types=None):
    """Genera un paso para la sección main.
    locals_refs: lista de nombres de locals
    locals_types: dict nombre → tipo ("bool", "num", "str")
    """
    paso = {}

    # Filtrar locals numéricos para statements/pass_fail que comparan
    num_refs = []
    if locals_types:
        num_refs = [k for k, v in locals_types.items() if v == "num"]

    # Tipo de paso
    r = random.random()
    if r < 0.55:
        # Paso gRPC normal
        paso["nombre"] = random.choice(PASOS_MAIN)
        if usar_reintentos:
            paso["reintentos"] = random.randint(1, 4)
        if usar_limite and random.random() < 0.4:
            lim = gen_limite()
            if lim:
                paso["limite"] = lim
    elif r < 0.70 and usar_statements and locals_refs:
        # Statement: asigna a un local existente
        paso["nombre"] = rand_name("stmt")
        paso["tipo"] = "statement"
        var = random.choice(locals_refs)
        var_type = locals_types.get(var, "num") if locals_types else "num"
        if var_type == "bool":
            val = random.choice(["true", "false"])
        elif var_type == "num":
            val = str(round(random.uniform(0, 10), 2))
        else:
            val = f'"{rand_word(4)}"'
        paso["statement"] = f"locals.{var} = {val}"
    elif r < 0.85 and usar_statements and num_refs:
        # pass_fail: sólo con variables numéricas
        paso["nombre"] = rand_name("veredicto")
        paso["tipo"] = "pass_fail"
        var = random.choice(num_refs)
        threshold = round(random.uniform(1, 8), 1)
        paso["condicion"] = f"locals.{var} > {threshold}"
    else:
        # Paso gRPC simple sin extras
        paso["nombre"] = random.choice(PASOS_MAIN)
        if usar_reintentos:
            paso["reintentos"] = random.randint(1, 3)

    # Precondición (sólo si hay locals numéricos o bool)
    if usar_precondicion and locals_refs and random.random() < 0.25:
        var = random.choice(locals_refs)
        var_type = locals_types.get(var, "num") if locals_types else "num"
        if var_type == "bool":
            paso["precondicion"] = f"locals.{var}"
        elif var_type == "num":
            threshold = round(random.uniform(0, 3), 1)
            paso["precondicion"] = f"locals.{var} > {threshold}"
        else:
            paso["precondicion"] = f'locals.{var} != ""'

    # disable
    if usar_disable and random.random() < 0.10:
        paso["disable"] = True

    # pause_on_fail
    if usar_pause and random.random() < 0.10:
        paso["pause_on_fail"] = True

    return paso


def gen_paso_setup():
    """Genera un paso para la sección setup."""
    paso = {"nombre": random.choice(PASOS_SETUP)}
    paso["reintentos"] = random.randint(1, 4)
    return paso


def gen_paso_cleanup():
    """Genera un paso para la sección cleanup."""
    paso = {"nombre": random.choice(PASOS_CLEANUP)}
    paso["reintentos"] = random.randint(1, 2)
    return paso


def gen_secuencia_basica():
    """Genera una secuencia básica con features aleatorias."""
    locals_dict, locals_types = gen_locals()
    locals_names = list(locals_dict.keys())

    sec = {
        "nombre": f"beta_{datetime.now().strftime('%H%M%S')}_{rand_word(4)}",
    }

    if locals_dict:
        sec["locals"] = locals_dict

    fg = gen_file_globals()
    if fg:
        sec["file_globals"] = fg

    # Setup (50% de las veces)
    if random.random() < 0.5:
        n_setup = random.randint(1, 3)
        sec["setup"] = [gen_paso_setup() for _ in range(n_setup)]

    # Main (siempre)
    n_main = random.randint(1, 6)
    sec["main"] = [
        gen_paso_main(
            locals_refs=locals_names if locals_names else None,
            locals_types=locals_types if locals_types else None,
        )
        for _ in range(n_main)
    ]

    # Cleanup (70% de las veces)
    if random.random() < 0.7:
        n_cleanup = random.randint(1, 2)
        sec["cleanup"] = [gen_paso_cleanup() for _ in range(n_cleanup)]

    return sec


def gen_secuencia_con_subsecuencias():
    """Genera una secuencia con subsecuencias inline."""
    # Siempre declarar los locals que el sequence_call necesita
    locals_dict = {"canal_in": round(random.uniform(0, 2), 2), "ok_init": False}
    locals_names = list(locals_dict.keys())

    sec = {
        "nombre": f"beta_sub_{datetime.now().strftime('%H%M%S')}_{rand_word(4)}",
        "locals": locals_dict,
    }

    # Subsecuencia inline
    sub_name = "init_comun"
    sec["subsecuencias"] = {
        sub_name: {
            "parameters": {"canal": 0.0, "listo": False},
            "main": [
                {
                    "nombre": "preparar_canal",
                    "tipo": "statement",
                    "statement": "parameters.listo = (parameters.canal >= 0.0)",
                }
            ],
        }
    }

    # Setup
    if random.random() < 0.5:
        sec["setup"] = [gen_paso_setup()]

    # Main con sequence_call
    main = []
    if random.random() < 0.7:
        main.append({
            "nombre": "preparar",
            "tipo": "sequence_call",
            "secuencia": sub_name,
            "parametros": {
                "canal": "locals.canal_in",
                "listo": "locals.ok_init",
            },
        })
    # Pasos normales
    n_main = random.randint(1, 4)
    locals_types = {"canal_in": "num", "ok_init": "bool"}
    for _ in range(n_main):
        main.append(gen_paso_main(locals_refs=locals_names, locals_types=locals_types))
    sec["main"] = main

    # Cleanup
    if random.random() < 0.5:
        sec["cleanup"] = [gen_paso_cleanup()]

    return sec


def gen_secuencia_con_veredicto():
    """Genera una secuencia con veredicto compuesto (pass_fail)."""
    sec = {
        "nombre": f"beta_veredicto_{datetime.now().strftime('%H%M%S')}_{rand_word(4)}",
        "locals": {
            "voltaje": 0.0,
            "led_ok": False,
        },
    }

    # Setup
    if random.random() < 0.5:
        sec["setup"] = [gen_paso_setup()]

    # Main con veredicto
    main = [
        {
            "nombre": "medir_voltaje",
            "reintentos": random.randint(1, 3),
            "limite": gen_limite_rango(),
            "asigna": {"voltaje": "${resultado.valor_medido}"},
        },
        {
            "nombre": "verificar_led",
            "reintentos": 1,
            "asigna": {"led_ok": '${resultado.estado == "paso"}'},
        },
        {
            "nombre": "verificar_dut",
            "tipo": "pass_fail",
            "condicion": "locals.voltaje > 3.5 && locals.led_ok",
        },
    ]

    # Añadir pasos extra aleatorios
    if random.random() < 0.3:
        main.insert(0, {
            "nombre": "abrir_rele",
            "reintentos": 1,
        })

    sec["main"] = main

    # Cleanup
    sec["cleanup"] = [gen_paso_cleanup()]

    return sec


def gen_secuencia_edge_case():
    """Genera casos límite para estresar al cargador/motor."""
    tipo = random.choice(["vacia_main", "solo_cleanup", "muchos_reintentos",
                           "limite_extremo", "precondicion_compleja"])

    if tipo == "vacia_main":
        # Main con un solo paso
        return {
            "nombre": f"beta_edge_min_{datetime.now().strftime('%H%M%S')}",
            "main": [{"nombre": "verificar_led", "reintentos": 1}],
        }

    elif tipo == "solo_cleanup":
        return {
            "nombre": f"beta_edge_cleanup_{datetime.now().strftime('%H%M%S')}",
            "main": [{"nombre": "verificar_led", "reintentos": 1}],
            "cleanup": [gen_paso_cleanup()],
        }

    elif tipo == "muchos_reintentos":
        return {
            "nombre": f"beta_edge_reintentos_{datetime.now().strftime('%H%M%S')}",
            "main": [
                {"nombre": "conectar_equipo", "reintentos": 10},
                {"nombre": "verificar_led", "reintentos": 1},
            ],
            "cleanup": [{"nombre": "desconectar_equipo", "reintentos": 1}],
        }

    elif tipo == "limite_extremo":
        return {
            "nombre": f"beta_edge_lim_{datetime.now().strftime('%H%M%S')}",
            "main": [
                {
                    "nombre": "medir_voltaje",
                    "reintentos": 1,
                    "limite": {"tipo": "rango", "min": 0.0, "max": 100.0},
                },
            ],
            "cleanup": [{"nombre": "desconectar_equipo", "reintentos": 1}],
        }

    else:  # precondicion_compleja
        return {
            "nombre": f"beta_edge_precond_{datetime.now().strftime('%H%M%S')}",
            "locals": {"activo": True, "contador": 0.0},
            "main": [
                {
                    "nombre": "init_log",
                    "tipo": "statement",
                    "statement": "locals.contador = 1.0",
                },
                {
                    "nombre": "medir_voltaje",
                    "reintentos": 1,
                    "precondicion": "locals.activo && locals.contador > 0.5",
                    "limite": gen_limite_rango(),
                },
                {
                    "nombre": "verificar_led",
                    "reintentos": 1,
                    "precondicion": "locals.activo",
                },
            ],
            "cleanup": [{"nombre": "desconectar_equipo", "reintentos": 1}],
        }


def rand_word(n):
    return "".join(random.choices(string.ascii_lowercase, k=n))


def generar_secuencia():
    """Genera una secuencia aleatoria eligiendo un tipo de plantilla."""
    plantilla = random.choice([
        gen_secuencia_basica,
        gen_secuencia_basica,
        gen_secuencia_basica,
        gen_secuencia_con_subsecuencias,
        gen_secuencia_con_veredicto,
        gen_secuencia_edge_case,
    ])
    return plantilla()


def generar_limites_sidecar(nombre_secuencia, pasos_main):
    """Genera un sidecar de límites para algunos pasos del main."""
    sidecar = {}
    for paso in pasos_main:
        if paso.get("nombre") == "medir_voltaje" and random.random() < 0.5:
            sidecar["medir_voltaje"] = gen_limite_comparacion()
    return sidecar if sidecar else None


def generar_flags_extra():
    """Genera flags extra aleatorios para la ejecución."""
    flags = []
    if random.random() < 0.6:
        flags.append("--json")
    if random.random() < 0.3:
        flags.append("--csv")
    if random.random() < 0.15:
        flags.append("--validate")
    return flags


def main():
    if SEED is not None:
        random.seed(SEED)
    else:
        random.seed()

    out_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/anvil-beta")
    out_dir.mkdir(parents=True, exist_ok=True)

    # Número de secuencias: argumento 2, o aleatorio 1-3
    if len(sys.argv) > 2:
        n_secuencias = int(sys.argv[2])
    else:
        n_secuencias = random.randint(1, 3)
    resultados = []

    for i in range(n_secuencias):
        sec = generar_secuencia()
        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        name = sec["nombre"]
        yaml_path = out_dir / f"{ts}_{name}.yaml"

        # Escribir la secuencia
        with open(yaml_path, "w") as f:
            yaml.dump(sec, f, default_flow_style=False, sort_keys=False,
                      allow_unicode=True)

        # Generar sidecar ocasionalmente
        sidecar_path = None
        if "main" in sec and random.random() < 0.2:
            sidecar = generar_limites_sidecar(name, sec["main"])
            if sidecar:
                sidecar_path = out_dir / f"{ts}_{name}.limits.yaml"
                with open(sidecar_path, "w") as f:
                    yaml.dump(sidecar, f, default_flow_style=False,
                              sort_keys=False, allow_unicode=True)

        resultado = {
            "timestamp": ts,
            "secuencia": name,
            "yaml_path": str(yaml_path),
            "sidecar_path": str(sidecar_path) if sidecar_path else None,
            "flags_extra": generar_flags_extra(),
        }
        resultados.append(resultado)

    # Escribir manifiesto
    manifiesto_path = out_dir / f"{datetime.now().strftime('%Y%m%d_%H%M%S')}_manifiesto.json"
    with open(manifiesto_path, "w") as f:
        json.dump({
            "generado": datetime.now().isoformat(),
            "n_secuencias": len(resultados),
            "secuencias": resultados,
        }, f, indent=2)

    print(json.dumps({
        "generado": datetime.now().isoformat(),
        "n_secuencias": len(resultados),
        "manifiesto": str(manifiesto_path),
        "secuencias": [str(r["yaml_path"]) for r in resultados],
    }, indent=2))


if __name__ == "__main__":
    main()