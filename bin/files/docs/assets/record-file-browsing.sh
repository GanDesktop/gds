#!/usr/bin/env bash
# Prepares its own demo window. See capture.md for dependencies and options.
set -euo pipefail
assets=$(cd -- "$(dirname -- "$0")" && pwd)
exec "${INPUT_PYTHON:-python3}" "$assets/record-demos.py" --scene browsing "$@"
