# BUG-1 — `--limits` se ignora cuando se usa `--process-model`

**Severidad:** alta. En producción el process model es obligatorio y el sidecar es
el mecanismo para cambiar umbrales por lote/variante (RF-30, ADR-0008). Con PM el
sidecar no se aplica, así que la secuencia corre con los límites embebidos sin que
nada lo advierta: un lote se puede dar por bueno con los umbrales equivocados.

**Reproducción — solo con ficheros oficiales del repo, sin nada de la beta:**

```sh
cd <raíz del repo>

# A) sin process model: el sidecar se aplica y la secuencia pasa
anvil ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml
#   stderr: sidecar de límites '...' aplicado (1 paso(s) afectado(s))
#   stdout: === limites: paso ===
#             [paso] medir_voltaje: medido: 4.2 V

# B) con process model: el mismo sidecar afecta a 0 pasos y la secuencia falla
anvil --process-model ejemplos/process_model_sequential.yaml \
        ejemplos/limites.yaml --limits ejemplos/limites.limits.yaml
#   stderr: sidecar de límites '...' aplicado (0 paso(s) afectado(s))
#   stdout: === process_model_sequential: fallo ===
#             [fallo] medir_voltaje: 4.2 fuera de rango [4.5, 5.5]
```

**Diagnóstico:** el sidecar se resuelve contra los pasos del **process model**
(`abrir_fixture`, `identificar_uut`, `test_uut`, `cerrar_fixture`), no contra los
de la secuencia del operador que el PM envuelve. Ningún nombre coincide, así que
no se sobreescribe ningún límite.

**Criterio de aceptación:** en el caso B el sidecar debe aplicarse a la secuencia
del operador (1 paso afectado) y el agregado debe ser `paso`, igual que en A.

**Nota relacionada:** un aviso cuando el sidecar afecta a 0 pasos habría hecho
este bug evidente de inmediato. Ver BUG-4 en el informe.

---

## Cómo verificarlo

```sh
./docs/qa/regresion/run.sh          # ejecuta los cuatro casos y compara con lo esperado
```
