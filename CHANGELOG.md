# Changelog

## Unreleased — 0.2.0

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
