#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
EXTENSION="$ROOT/build/release/units.duckdb_extension"
if [[ ! -f "$EXTENSION" ]]; then
    echo "Build first: cd '$ROOT' && make configure && make release" >&2
    exit 1
fi
if [[ -n "${DUCKDB_BIN:-}" ]]; then
    CLI=$DUCKDB_BIN
elif [[ -x "$HOME/.duckdb/cli/2.0.0/duckdb" ]]; then
    CLI="$HOME/.duckdb/cli/2.0.0/duckdb"
else
    CLI=$(command -v duckdb || true)
fi
if [[ -z "$CLI" ]]; then
    echo 'DuckDB 2.0.0 is required. Set DUCKDB_BIN=/absolute/path/to/duckdb.' >&2
    exit 1
fi
# Absolute paths are required by hardened macOS DuckDB builds. Escape SQL quotes.
EXTENSION=${EXTENSION//\'/\'\'}
exec "$CLI" -no-agent -unsigned -bail -cmd "LOAD '$EXTENSION';" < "$ROOT/examples/demo.sql"
