# units — physical units for DuckDB, in Rust

A native DuckDB **2.0.0 / C Extension API v2** extension for unit conversion,
compound-unit parsing, dimensional compatibility, unit discovery, and auditable measurement normalization.

**Status: 0.3.0 development demo, not a production release.** No worker process or custom
numeric type. Not yet published to the DuckDB community registry.

Built from [duckdb/extension-template-rs](https://github.com/duckdb/extension-template-rs),
using `duckdb-neo = 0.20000.1` and
[`octofhir-ucum = 0.9.2`](https://github.com/octofhir/ucum-rs).
Unit syntax and definitions follow **UCUM 2.2**, case-sensitive. The upstream engine
reports UCUM conformance; this extension intentionally exposes a smaller conversion
surface and does **not** claim independently certified/full UCUM compliance.

## Run the demo

Prerequisites: current stable Rust, Python 3 with venv, Make, Git, and a DuckDB 2.0.0 CLI.
No DuckDB source build, C++ compilation, or separately installed unit database is needed.

```sh
git clone --recurse-submodules https://github.com/sebastiaan-dev/duckdb-units-rs.git
cd duckdb-units-rs
make configure
make release
make test_release
./scripts/demo.sh
```

The launcher uses `DUCKDB_BIN` if set, then `~/.duckdb/cli/2.0.0/duckdb`, then `duckdb`
on PATH. To select a CLI explicitly:

```sh
DUCKDB_BIN=/path/to/duckdb ./scripts/demo.sh
```

`make demo` builds the release extension and runs the same demonstration.
The complete SQL is in [`examples/demo.sql`](examples/demo.sql).

For an interactive session:

```sh
~/.duckdb/cli/2.0.0/duckdb -unsigned \
  -cmd "LOAD '$PWD/build/release/units.duckdb_extension';"
```

Use an **absolute extension path** on macOS. The unstable C API requires the exact
DuckDB target version; a 1.x or different 2.x binary is not a supported substitute.
`-unsigned` is needed for a locally built extension.

## Examples

```sql
-- Normalize units stored in a column.
SELECT site, unit_convert(value, source_unit, 'kW.h') AS kwh
FROM (VALUES
    ('Plant A', 3600000.0, 'J'),
    ('Plant B', 1250.0, 'W.h'),
    ('Plant C', 2.5, 'kW.h')
) AS energy_readings(site, value, source_unit);
-- Plant A: 1.0; Plant B: 1.25; Plant C: 2.5

-- Real compound expressions, not a fixed list of dimension categories.
SELECT unit_convert(1000, 'kg/m3', 'g/L');          -- 1000
SELECT unit_compatible('Pa', 'kg/(m.s2)');         -- true
SELECT unit_compatible('kg', 'kW.h');              -- false

-- Absolute temperatures versus differences: explicit, separate APIs.
SELECT unit_convert(20, 'Cel', '[degF]');          -- 68
SELECT unit_convert_delta(20, 'Cel', '[degF]');    -- 36
SELECT unit_convert(20, 'Cel', 'K');               -- 293.15
SELECT unit_convert_delta(20, 'Cel', 'K');         -- 20

-- Strict conversions fail loudly.
SELECT unit_convert(1, 'kg', 'm');                -- ERROR: incompatible_dimension

-- Checked conversions preserve data quality information instead of aborting.
SELECT unit_convert_checked(1, 'kg', 'm');
-- {'value': NULL, 'status': 'incompatible_dimension', 'message': ...}
SELECT unit_convert_checked(NULL, 'J', 'kW.h');
-- {'value': NULL, 'status': 'missing_input', 'message': ...}
```

### Syntax matters

| Meaning | Accepted UCUM spelling | Not an alias in this release |
|---|---|---|
| Kilowatt-hour | `kW.h` | `kWh` |
| Density | `kg/m3` | `kg/m^3` |
| Acceleration | `m/s2` | `m/s^2` |
| Celsius | `Cel` | `C`, `°C`, `celsius` |
| Fahrenheit | `[degF]` | `F`, `°F` |
| Micrometre | `um` | `µm` |
| International inch | `[in_i]` | `inch` |
| Dimensionless | `1` | empty string |

In unit-string APIs there is no case folding, whitespace trimming, or heuristic alias
guessing. **`C` means coulomb, not Celsius; `F` means farad, not Fahrenheit.** `MW` and
`mW` differ by a factor of one billion. Use parentheses to make denominators explicit.

UCUM annotations are accepted but do not enforce quantity kinds: `mg{total}` and `mg`
are dimensionally equivalent. Annotations must not be used as a substitute for validation.

## Discover units and audit unit columns

```sql
SELECT unit, name, property, supported
FROM unit_search('pressure') ORDER BY unit;
-- Includes Pa, bar, atm, [psi], ... and unsupported B[SPL], clearly marked.

SELECT c.unit, c.factor, c.offset, c.base_unit
FROM units_catalog() c WHERE unit IN ('Cel', 'Pa');

SELECT unit_parse_checked('kg/m3');
-- {definition: {dimension: 'L-3.M', base_unit: 'm-3.g', factor: 1000,
--               offset: 0, is_affine: false}, status: 'ok', message: NULL}

SELECT unit_parse_checked('Cel/s');
-- {definition: NULL, status: 'unsupported_unit', message: ...}
```

The catalog contains **312 registered UCUM atoms**, of which **253 currently pass this
extension's conversion policy**. This is not the number of supported expressions: prefixes,
products, quotients, and powers generate many more, and are not enumerated as separate rows.
Unsupported atoms remain visible with diagnostics; their parsed-definition columns are NULL.

Both discovery tables have the same columns:

- `unit`, `name`, `property`: the code, display name, and descriptive property from UCUM.
- `prefixable`, `is_base`: registry flags; `prefixable` means the atom accepts UCUM prefixes.
- `supported`, `status`, `message`: the extension's current support decision.
- `dimension`, `base_unit`, `factor`, `offset`, `is_affine`: the same definition as `unit_parse`,
  or NULL when unsupported. Qualify `offset` as `c.offset` or quote it in SQL.

`property` is descriptive metadata, **not enforced quantity-kind validation**.
`unit_search(query)` uses case-insensitive substring matching over codes, names, and properties.
It is **not alias resolution**: finding `Cel` with `unit_search('celsius')` does not make
`unit_convert(20, 'celsius', 'K')` valid. Search does not trim whitespace or interpret wildcards.
An empty query lists all atoms; NULL returns zero rows. Queries are limited to 4096 bytes.
Search arguments must be constant at binding/execution time (prepared parameters work),
not correlated unit columns. Use `ORDER BY` when result order matters.

To profile a dirty unit column without requiring measurement values:

```sql
WITH inputs(u) AS (
    VALUES ('kg/m3'), ('Cel'), ('Cel/s'), ('madeup'), (NULL)
), audited AS (
    SELECT u, unit_parse_checked(u) AS result FROM inputs
)
SELECT result.status, count(*) AS rows
FROM audited GROUP BY result.status ORDER BY result.status;
```

## Parse measurements stored as text

```sql
SELECT quantity_parse('12.5 kg/m3');
-- {value: 12.5, unit: 'kg/m3', definition: {dimension: 'L-3.M', ...}}

SELECT quantity_parse_checked('-1.25E+3 J');
-- {quantity: {value: -1250, unit: 'J', definition: {...}}, status: 'ok', message: NULL}

SELECT quantity_parse_checked('1eV');
-- {quantity: NULL, status: 'invalid_quantity', message: ...}
SELECT quantity_parse('1 eV'); -- one electronvolt, unambiguously
```

Quantity text has a deliberately explicit grammar:

- A decimal number, **at least one ASCII space or tab**, and a supported UCUM unit.
- An optional leading `+` or `-`, a decimal point, and an `e`/`E` exponent are accepted:
  `+.5 m`, `2. m`, and `-1.25E+3 J` are valid. Group separators, decimal commas,
  hexadecimal numbers and locale guessing are not supported.
- Surrounding ASCII spaces/tabs are ignored for the quantity envelope. Newlines and
  Unicode whitespace separators are rejected. Unit syntax itself remains strict.
- Compact forms such as `12.5kg` and `1eV` are rejected rather than guessed. A unit is
  mandatory; use `3 1` for a dimensionless measurement, not just `3`.
- Values must fit in a finite DOUBLE. NaN/Infinity report `non_finite_input`; out-of-range
  decimal values and nonzero values rounded to zero report `overflow`. Representable
  subnormals and genuine signed zero are accepted.
- Total quantity text is limited to 8192 bytes; the unit expression still has a 4096-byte
  limit. Numeric-envelope validation precedes unit resolution.

The result preserves the value and unit spelling after trimming envelope whitespace; it
**does not convert to base units**. `definition` is the same struct returned by `unit_parse`.
Parsing `20 Cel` does not decide whether the value is an absolute temperature or a difference;
choose `unit_convert` or `unit_convert_delta` explicitly when converting it.

The checked form returns `STRUCT(quantity, status, message)`. On any error, the entire
`quantity` and its nested fields are NULL. Unit-parser errors retain their original status
and message. NULL input is `missing_input`; a malformed numeric envelope is `invalid_quantity`.

For dirty text, retain parsing errors and use checked conversion for the next stage too:

```sql
WITH inputs(raw) AS (
    VALUES ('3600000 J'), ('1250 W.h'), ('2 kg'), ('1e J'), (NULL)
), parsed AS (
    SELECT raw, quantity_parse_checked(raw) AS p FROM inputs
), converted AS (
    SELECT *, CASE WHEN p.status='ok'
        THEN unit_convert_checked(p.quantity.value, p.quantity.unit, 'kW.h') END AS c
    FROM parsed
)
SELECT raw, CASE WHEN p.status='ok' THEN c.status ELSE p.status END AS status,
       round(c.value, 6) AS kwh
FROM converted;
```

## SQL API

All measurement arguments/results are `DOUBLE`; unit arguments are `VARCHAR`.

| Function | Result / contract |
|---|---|
| `unit_convert(value, source_unit, target_unit)` | Strict multiplicative/absolute-temperature conversion |
| `unit_convert_delta(value, source_unit, target_unit)` | Strict temperature-difference conversion; no offsets; temperature dimensions only |
| `unit_convert_checked(value, source_unit, target_unit)` | `STRUCT(value DOUBLE, status VARCHAR, message VARCHAR)` |
| `unit_convert_delta_checked(value, source_unit, target_unit)` | Same diagnostic struct, for temperature differences |
| `unit_compatible(source_unit, target_unit)` | Boolean dimensional compatibility; unknown/unsupported units raise errors |
| `unit_parse(unit)` | `STRUCT(dimension VARCHAR, base_unit VARCHAR, factor DOUBLE, offset DOUBLE, is_affine BOOLEAN)` |
| `unit_parse_checked(unit)` | `STRUCT(definition <unit_parse result>, status VARCHAR, message VARCHAR)`; failed/missing input has NULL definition |
| `quantity_parse(text)` | `STRUCT(value DOUBLE, unit VARCHAR, definition <unit_parse result>)` |
| `quantity_parse_checked(text)` | `STRUCT(quantity <quantity_parse result>, status VARCHAR, message VARCHAR)` |
| `unit_dimension(unit)` | UCUM dimension vector as text |
| `unit_valid(unit)` | Whether the unit is supported by **this extension**, not a full UCUM syntax validator |
| `units_backend()` | Backend, UCUM, and extension-policy versions |
| `units_catalog()` | Table: all registered atoms, definitions, and support diagnostics |
| `unit_search(query)` | Table: case-insensitive discovery, same schema as `units_catalog()` |

Function descriptions and examples are also in `duckdb_functions()`:

```sql
SELECT function_name, description
FROM duckdb_functions() WHERE categories = ['units'];
```

### Parsed representation

```sql
SELECT unit_parse('kg/m3');
-- dimension: L-3.M; base_unit: m-3.g; factor: 1000; offset: 0; is_affine: false
```

`base_value = value * factor + offset`. UCUM base units are `m`, `s`, **`g`**, `rad`,
`K`, `C`, and `cd`; these are not the seven SI base units. Dimension labels are `L`
(length), `T` (time), `M` (mass), `A` (plane angle), `Θ` (temperature), `Q` (charge),
and `F` (luminous intensity). `1` is dimensionless.

`base_unit` is the dimensional base expression, not a canonical spelling preserving
scale or offset. Do **not** replace a unit with this string without transforming its value.
UCUM treats plane angle as a separate dimension, so `Hz` and `rad/s` are not compatible.

### Errors and NULLs

Strict scalar functions propagate any NULL input to NULL. `unit_valid(NULL)` is also NULL.
Checked functions always return a struct: missing inputs produce `missing_input`, even
if another argument is invalid. A successful struct has `status = 'ok'` and NULL message.
Failed conversions have NULL `value`; failed unit parsing has NULL `definition`; failed
quantity parsing has NULL `quantity`. Nested fields are NULL as well. All checked functions
have a stable status code and explanatory text. Discovery table NULL behavior is described above.

Statuses include `missing_input`, `invalid_quantity`, `invalid_syntax`, `unknown_unit`,
`incompatible_dimension`, `unsupported_unit`, `invalid_delta_unit`,
`non_finite_input`, `overflow`, and `resource_limit`. Match status codes, not message text.
The `overflow` status also covers numerical underflow to zero.

## Implementation and boundaries

- Parsing/definitions come from a pinned UCUM library, not hand-written conversion macros.
- Native vector callbacks handle runtime unit columns and NULL/filtered inputs.
- Each query worker has bounded caches: up to 256 parsed units and 256 conversion pairs,
  including error results. Caches clear when full; user inputs/results are not retained globally.
  Dynamic units are parsed on first execution. Constant source/target conversion units are
  prepared at bind time, with invalid plans deferred until execution to preserve NULL/error
  semantics. Quantity parsing caches the unit definition, not the entire measurement text.
- Discovery tables share an immutable cache of the library's atom definitions and support
  decisions. Table scans have independent cursors, exact cardinality, and projection pushdown.
  Filters remain evaluated by DuckDB; search does not mutate the registry or parser.
- Ordinary conversions use precomputed scale/offset and fused multiply-add. Extreme ratios
  and numerical boundary results use the backend's wider decimal arithmetic before narrowing
  the final value to `DOUBLE`. Decimal factors have up to 28 significant digits; SQL results
  are still floating-point, not exact rational quantities.
- Input unit strings are limited to 4096 bytes. The backend limits nesting to 128 and rejects
  dimensional exponent overflow. Unit factors must be positive, finite and nonzero when
  represented as `DOUBLE`. Non-finite measurements/results and nonzero results rounded to zero
  are rejected. This is not arbitrary-precision SQL arithmetic.
- Multiplicative expressions and affine temperatures are supported. Logarithmic and arbitrary
  units are deliberately rejected. Affine temperature products/quotients/powers such as `Cel/s`
  are rejected; use `K/s` for temperature-rate expressions. Delta conversion applies to standalone
  temperature dimensions, not arbitrary compound expressions.
- Compatibility checks dimensions, **not semantic quantity kinds**: energy and torque are
  dimensionally compatible. The extension does not infer what a measurement represents, enforce
  absolute zero, propagate uncertainty, or validate physical plausibility.
- No custom numeric type, quantity algebra, or alias registry yet.
- Native macOS arm64 has been exercised locally. The template retains WASM and distribution
  wiring, but local success is not a claim of validation on every platform.

## Performance

The [benchmark report](benchmarks/README.md) covers all function families, including reproducible
Hyperfine commands, workload definitions, build provenance, and measurement limitations. Targeted
optimizations reduced constant-unit conversion latency by about 30–33% and unique-unit parsing /
validation by about 15% on the measured single-thread workload, without weakening validation.
These are workload-specific results, not universal speedup guarantees.

## Development

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
make release
make test_release
python3 scripts/test_build_config.py
configure/venv/bin/python3 scripts/test_build_metadata.py
```

The build delegates to Cargo; Make configures the SQL test environment and adds the DuckDB
extension metadata footer. `make debug && make test_debug` is also supported. Each build
refreshes revision metadata (or the explicit `EXTENSION_VERSION` override) before packaging;
rebuilding no longer reuses a stale revision from an earlier `make configure`. The metadata
regression test rebuilds and loads the extension in isolated processes; run it serially, not
alongside another build of the same output directory.

Layout:

- `crates/engine/`: DuckDB-independent parsing/conversion policy, immutable atom catalog, and tests.
- `crates/core/`: SQL registration, scalar/table callbacks, checked structs, and bounded caches.
- `crates/native/`: native `units` entrypoint.
- `crates/wasm/`: shared-core WASM entrypoint inherited from the template.
- `test/sql/units.test`: end-to-end SQLLogicTest coverage, including mixed-unit columns,
  temperature semantics, diagnostics, high-cardinality inputs, and numerical regressions.
- `test/sql/discovery.test`: registry/parser consistency, discovery queries, table projections,
  repeated/prepared scans, and checked nested-struct validity across chunks.
- `test/sql/quantity.test`: quantity envelopes, exponent/locale ambiguity, numerical boundaries,
  exact resource limits, and strict/checked nested NULL behavior.
- `test/sql/bind.test`: constant-unit specialization, deferred errors, CASE/TRY, prepared
  parameters, optimizer equality/grouping, and filtered multi-chunk input.
- `benchmarks/run.py`: output-verified, reproducible Hyperfine comparisons.
- `scripts/test_build_config.py`: WASM v2 link-command regression checks. These dry-run checks
  complement actual builds in distribution CI; they are not WASM runtime tests.

See [the roadmap](docs/ROADMAP.md) for the remaining release gates and planned scope.

For a DuckDB version upgrade, update the workspace binding dependency, entrypoint API versions,
Makefile target, distribution workflow and CI-tools submodule together, then clean/reconfigure
and rerun SQL tests. Cross-version ABI compatibility is not assumed.

## License

Project code is licensed under the [MIT License](LICENSE).
Third-party dependencies and UCUM data retain their respective license terms.
