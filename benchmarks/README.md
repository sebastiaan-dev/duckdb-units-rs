# Function benchmarks

## Summary (2026-11-02)

The extension was already fast for repeated unit strings. Two optimizations were worthwhile:

1. **Prepare constant source/target conversions at bind time.** Avoid per-row string reads and
   cache lookups. Store invalid plans as data rather than raising errors during binding, preserving
   NULL, empty-scan, CASE, TRY and checked-result behavior.
2. **Reduce parsing work without relaxing validation.** Render the base-unit expression directly
   into one string, and bypass decimal formatting/multiplication for exact identity factors.

Cached dynamic conversion now evaluates plans by reference instead of copying their wider decimal
coefficients per row. A first candidate showed a possible mixed-unit regression; focused rechecks
and this change restored approximately baseline performance on that path.

### Method

- Host: Apple M5 Pro, 18 cores, 48 GiB RAM; macOS Darwin 25.6.0 arm64.
- DuckDB CLI: 2.0.0, `d54cbbe3df`.
- Compiler: Rust 1.98.1 (Homebrew), LLVM 22.1.8.
- Both builds: `make release`, Cargo release, `lto = true`, `strip = true`, default target flags.
- Baseline: `0a9d7cf1ea3b458fcb98fbcf31a1778f4ef234be`, clean tree.
- Optimized: `b8d1b31bcd35b9721ff82142be13fefeae146881`, clean tree.
- Hyperfine 1.20.0; **15 measured samples and 3 warmups per command**; each matched pair adjacent.
- **1,000,000 prebuilt rows and one DuckDB thread** for row-function workloads. Data setup is outside
  timing. The final three cases instead execute **100 small queries** per sample.
- Fresh CLI and application caches per sample, warmed filesystem cache. Timings include process
  startup, extension loading, input scanning, output construction and result aggregation.
- Results are consumed using SUM or a hash of the whole struct. Outputs were byte-identical for
  all 18 baseline/optimized workloads before timing; all measured exits succeeded.
- This was a developer workstation, not an isolated host. Values below are mean ± **sample SD**,
  not confidence intervals. Raw samples were retained locally, not committed.

### Results

All times are milliseconds per complete command; lower is better.

| Workload | Baseline, mean ± SD | Optimized, mean ± SD |
|---|---:|---:|
| Scan/startup control | 31.3 ± 3.2 | 33.1 ± 4.1 |
| `unit_convert`, constant units | **53.6 ± 7.0** | **36.5 ± 8.3** |
| `unit_convert`, mixed source column | 64.1 ± 3.4 | 62.4 ± 5.3 |
| `unit_convert_checked`, dirty source column | 91.2 ± 6.8 | 90.0 ± 4.9 |
| `unit_convert_delta`, constant units | **57.4 ± 6.7** | **38.4 ± 5.2** |
| `unit_convert_delta_checked`, constant units | **78.1 ± 4.3** | **54.8 ± 6.3** |
| `unit_compatible` | 66.3 ± 4.7 | 65.0 ± 5.3 |
| `unit_dimension` | 48.0 ± 4.6 | 51.0 ± 3.7 |
| `unit_valid`, repeated units | 51.1 ± 4.7 | 51.0 ± 5.6 |
| `unit_valid`, unique units | **291.6 ± 7.1** | **248.8 ± 14.8** |
| `unit_parse`, repeated units | 86.2 ± 3.8 | 87.3 ± 4.9 |
| `unit_parse`, unique units | **328.0 ± 12.4** | **278.2 ± 5.6** |
| `unit_parse_checked`, dirty units | 109.8 ± 3.8 | 109.5 ± 3.8 |
| `quantity_parse` | 116.2 ± 5.0 | 122.4 ± 4.2 |
| `quantity_parse_checked` | 174.4 ± 41.1* | 126.3 ± 3.2 |
| `units_backend`, **100 calls** | 34.3 ± 3.0 | 35.1 ± 1.1 |
| `units_catalog`, **100 calls** | 44.5 ± 2.5 | 44.7 ± 2.1 |
| `unit_search`, **100 calls** | 44.2 ± 1.7 | 45.4 ± 1.8 |

*The checked-quantity baseline had unusually high variance in this run. Earlier surveys put it
around 128–134 ms. No optimization speedup is claimed for quantity parsing, metadata/discovery,
or other small changes in unaffected paths. In particular, the ~5% higher quantity-parse wall
time is reported, not hidden; its measured user CPU time was approximately unchanged and repeat
surveys varied by a similar amount. More controlled measurement would be needed for small claims.

The targeted reductions are approximately **30–33% for constant-unit conversion paths** and
**15% for unique-unit validation/parsing**, including the ~30 ms process/load/scan overhead.
Independent shorter experiments also observed these improvements. These are workload-specific
latency changes, not universal speedup guarantees.

### Workload details and limitations

- Values are finite nonzero DOUBLEs; repeated units cycle through `J`, `W.h`, `kW.h`, and `MJ`.
- Unique units are distinct `m{row_i}` annotations. This stresses cache misses without changing
  their common physical dimension. It does not represent every possible compound expression.
- Dirty conversion has 5% NULL units, 5% `kg` (incompatible with `J`), and 5% `watts` (unknown).
  In checked *parsing*, `kg` is valid. Dirty quantity text has 5% NULL, 5% malformed numeric text,
  and 5% NaN text. Checked delta uses valid constant temperature units.
- No multithread scaling, cold-disk, enormous-expression, or rare wide-arithmetic-fallback claims
  are made here. Those need separate workloads.
- Discovery is not a throughput hotspot: even 100 queries completed in about 35–45 ms including
  one process startup. Repeated unit parsing and quantity parsing were left semantically unchanged.

## Profiling and correctness

Samply profiling of unique-unit parsing showed most work under unit parsing, with temporary
string construction and decimal conversion visible. Profiling used a separate symbol-bearing
release build (`debug=1`, `strip=none`, frame pointers) and a locally signed copy of the CLI;
the timed benchmarks used the normal release artifacts and the original CLI.

Rust tests and SQLLogicTests pass. `test/sql/bind.test` also passes against the unchanged baseline.
Differential checks covered NULLs, invalid units, CASE, TRY, folding failures, prepared parameters,
optimizer equality/grouping, temperature offsets/deltas, numeric limits, and multi-chunk input.
No manual unsafe code, case folding, weakened unit validation, or changed numeric rounding were added.

## Reproduce

Build the two desired revisions with the same toolchain and flags, then run on a POSIX host:

```sh
python3 benchmarks/run.py \
  --duckdb "$HOME/.duckdb/cli/2.0.0/duckdb" \
  --baseline /absolute/path/to/baseline/units.duckdb_extension \
  --optimized "$PWD/build/release/units.duckdb_extension" \
  --baseline-revision 'BASELINE_SHA and actual dirty/clean state' \
  --optimized-revision 'OPTIMIZED_SHA and actual dirty/clean state' \
  --build-notes 'actual compiler version, build profile and flags' \
  --output /tmp/units-bench-new-run \
  --rows 1000000 --threads 1 --runs 15 --warmup 3
```

The output directory must be new or empty. It receives copied binaries, their SHA256 hashes,
setup SQL, per-case SQL, an environment/provenance manifest, result checksums, and the Hyperfine
JSON export. `--cases` selects a comma-separated subset of case names defined in `run.py`.
Record actual metadata: an embedded revision alone does not prove the working tree was clean.

The default correctness check also compares `units_backend()` output. When comparing different
policy versions, omit `backend_100` from `--cases` rather than disregarding all output differences.
For analysis, import the Hyperfine export with the benchmark-results tooling; retain the raw
samples and distinguish row workloads from the 100-call metadata batches.
