#!/usr/bin/env sh
set -eu
: "${SAIR_ACC_ROOT:?set SAIR_ACC_ROOT to the official SAIR checkout}"
PYTHONPATH="$SAIR_ACC_ROOT/competition/tools" exec python3 -m verifier "$@"
