#!/bin/bash
# Wrapper para el cron del betatester de Anvil.
# Genera secuencias, las corre y guarda logs.

set -e
cd /media/alaforga/ssd/01-PRODUCTOS/Anvil

python3 betatester/run.py --n 5 --timeout 60 2>&1 | tee -a betatester/logs/cron.log