#!/usr/bin/env python3
"""Reproducible POSIX Hyperfine matrix; raw data stays in the chosen output directory."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess


CASES = {
    "scan": "SELECT sum(v) FROM measurements;",
    "convert_constant": "SELECT sum(unit_convert(v,'W.h','J')) FROM measurements;",
    "convert_mixed": "SELECT sum(unit_convert(v,u,'J')) FROM measurements;",
    "convert_checked": "SELECT bit_xor(hash(unit_convert_checked(v,dirty_unit,'J'))) FROM measurements;",
    "convert_delta": "SELECT sum(unit_convert_delta(v,'Cel','[degF]')) FROM measurements;",
    "delta_checked": "SELECT bit_xor(hash(unit_convert_delta_checked(v,'Cel','[degF]'))) FROM measurements;",
    "compatible": "SELECT sum(unit_compatible(u,'J')::UBIGINT) FROM measurements;",
    "dimension": "SELECT bit_xor(hash(unit_dimension(u))) FROM measurements;",
    "valid_cached": "SELECT sum(unit_valid(u)::UBIGINT) FROM measurements;",
    "valid_unique": "SELECT sum(unit_valid(unique_unit)::UBIGINT) FROM measurements;",
    "parse_cached": "SELECT bit_xor(hash(unit_parse(u))) FROM measurements;",
    "parse_unique": "SELECT bit_xor(hash(unit_parse(unique_unit))) FROM measurements;",
    "parse_checked": "SELECT bit_xor(hash(unit_parse_checked(dirty_unit))) FROM measurements;",
    "quantity_parse": "SELECT bit_xor(hash(quantity_parse(quantity))) FROM measurements;",
    "quantity_checked": "SELECT bit_xor(hash(quantity_parse_checked(dirty_quantity))) FROM measurements;",
    # Tiny discovery calls are batched. They are NOT million-row throughput cases.
    "backend_100": "SELECT units_backend();\n" * 100,
    "catalog_100": "SELECT bit_xor(hash(c)) FROM units_catalog() c;\n" * 100,
    "search_100": "SELECT bit_xor(hash(c)) FROM unit_search('pressure') c;\n" * 100,
}


def sql_literal(text):
    return "'" + str(text).replace("'", "''") + "'"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--duckdb", required=True, type=Path)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--optimized", required=True, type=Path)
    parser.add_argument("--baseline-revision", required=True, help="Include dirty-tree state")
    parser.add_argument("--optimized-revision", required=True, help="Include dirty-tree state")
    parser.add_argument("--build-notes", required=True, help="Actual compiler, profile and flags")
    parser.add_argument("--output", required=True, type=Path, help="New or empty directory, preferably outside the repo")
    parser.add_argument("--rows", type=int, default=1_000_000)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--runs", type=int, default=12)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--cases", default=",".join(CASES))
    args = parser.parse_args()
    if min(args.rows, args.threads, args.runs) < 1 or args.warmup < 0:
        parser.error("rows, threads and runs must be positive; warmup must be nonnegative")
    names = args.cases.split(",")
    if len(names) != len(set(names)) or any(name not in CASES for name in names):
        parser.error("cases must be unique known names: " + ",".join(CASES))
    cli = args.duckdb.resolve(strict=True)
    source_paths = {v: getattr(args, v).resolve(strict=True) for v in ("baseline", "optimized")}
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("output directory must be empty; refusing to overwrite a previous experiment")
    output.mkdir(parents=True, exist_ok=True)
    database = output / "input.duckdb"
    setup = f"""SET threads={args.threads};
CREATE TABLE measurements AS
SELECT i, (i%1000+1)::DOUBLE/10 AS v,
       CASE i%4 WHEN 0 THEN 'J' WHEN 1 THEN 'W.h' WHEN 2 THEN 'kW.h' ELSE 'MJ' END AS u,
       'm{{row_'||i||'}}' AS unique_unit,
       CASE i%20 WHEN 0 THEN NULL WHEN 1 THEN 'kg' WHEN 2 THEN 'watts' ELSE u END AS dirty_unit,
       v::VARCHAR||' '||u AS quantity,
       CASE i%20 WHEN 0 THEN NULL WHEN 1 THEN '1e J' WHEN 2 THEN 'NaN J' ELSE quantity END AS dirty_quantity
FROM range({args.rows}) t(i);
CHECKPOINT;
"""
    (output / "setup.sql").write_text(setup)
    subprocess.run([str(cli), "-no-agent", "-bail", str(database)], input=setup, text=True, check=True, capture_output=True)
    manifest = {
        "rows": args.rows, "threads": args.threads, "warmup": args.warmup, "runs": args.runs,
        "build": args.build_notes, "platform": platform.platform(), "machine": platform.machine(),
        "cpu_count": os.cpu_count(), "cases": names,
        "duckdb_version": subprocess.check_output([str(cli), "--version"], text=True).strip(),
        "hyperfine_version": subprocess.check_output(["hyperfine", "--version"], text=True).strip(),
        "inputs": {}, "checksums": {},
        "conditions": "Fresh CLI/application caches; warm filesystem cache; setup outside timing; results consumed; stdout discarded. Discovery cases execute 100 queries per sample.",
    }
    commands = [str(cli), "-csv", "-noheader", "-no-agent", "-unsigned", "-readonly", str(database)]
    for variant, source in source_paths.items():
        directory = output / variant
        directory.mkdir()
        binary = directory / "units.duckdb_extension"
        shutil.copyfile(source, binary)
        manifest["inputs"][variant] = {
            "source": str(source), "revision": getattr(args, variant + "_revision"),
            "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        }
        for name in names:
            query = f"SET threads={args.threads};\nLOAD {sql_literal(binary)};\n{CASES[name]}\n"
            (directory / (name + ".sql")).write_text(query)
            # Check semantic equivalence BEFORE timing; consume every scalar/struct result.
            result = subprocess.check_output(commands, input=query.encode())
            digest = hashlib.sha256(result).hexdigest()
            if variant == "baseline":
                manifest["checksums"][name] = digest
            elif digest != manifest["checksums"][name]:
                raise RuntimeError(f"Baseline/optimized result mismatch in {name}")
    measured = (
        f"{shlex.quote(str(cli))} -no-agent -unsigned -readonly {shlex.quote(str(database))} "
        f"< {shlex.quote(str(output))}/{{variant}}/{{case}}.sql > /dev/null"
    )
    invocation = [
        "hyperfine", "--warmup", str(args.warmup), "--runs", str(args.runs),
        "--export-json", str(output / "hyperfine.json"),
        # Hyperfine expands the first parameter innermost: keep each matched pair adjacent.
        "-L", "variant", "baseline,optimized", "-L", "case", ",".join(names), measured,
    ]
    manifest["invocation"] = invocation
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Verified identical results in {len(names)} cases. Starting Hyperfine.", flush=True)
    subprocess.run(invocation, check=True)


if __name__ == "__main__":
    main()
