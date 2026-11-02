# Changelog

## Unreleased — 0.3.0

- Specialize constant-unit conversions at bind time while retaining execution-time errors and
  NULL semantics; evaluate cached dynamic plans by reference rather than copying coefficients.
- Reduce parser allocations and exact-identity arithmetic; add reproducible Hyperfine benchmarks
  and a report of measured improvements and limitations.

- Add strict `quantity_parse(text)` and diagnostic `quantity_parse_checked(text)` for a
  decimal measurement followed by an explicit space/tab and a supported UCUM expression.
- Preserve original unit spelling and return the parsed definition without silently converting
  the measurement, inferring a unit, or choosing absolute versus delta semantics.
- Reject ambiguous compact forms, locale-dependent numbers, non-finite values, and numeric
  overflow/underflow to zero; enforce bounded input lengths.
- Extend the demo and SQL tests with text normalization, precise numerical/resource boundaries,
  unchanged unit-error diagnostics, and nested NULL handling across chunks.

## Development milestone — 0.2.0

- Update pinned CI tools to export the v2 entrypoint when linking WASM; add regression
  checks for the link configuration of all three WASM variants in debug and release.
- License the project under MIT and declare it in all Rust workspace packages.
- Add `units_catalog()` and `unit_search(query)` table functions over the pinned UCUM atom
  registry. Unsupported units stay visible with status/reason and NULL conversion definitions.
- Add `unit_parse_checked(unit)`, returning a nested definition plus status/message without
  aborting on invalid or missing input. Strict parsing and conversion semantics are unchanged.
- Add projection-aware catalog scans, independent scan state, and tests for repeated/prepared
  executions and registry/parser consistency.
- Extend the demo with unit discovery and unit-column auditing.
- Fix stale binary revision metadata after local rebuilds or changes to `EXTENSION_VERSION`.
- Run release SQL tests and an E2E metadata-refresh regression test in Rust CI.
- Document remaining release gates and deliberately deferred features.

## Initial demo — 0.1.0

- Native DuckDB 2.0.0 C Extension API v2 implementation with a pinned UCUM backend.
- Conversion, explicit temperature-difference conversion, checked diagnostics, parsing,
  dimensional compatibility, and worker-local caches.
- Numerical boundary fallback and end-to-end SQL tests.
