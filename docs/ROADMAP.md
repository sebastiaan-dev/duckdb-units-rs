# Roadmap and release gates

This is a development extension, not a published community extension. The priority is
trustworthy measurement normalization using ordinary DuckDB columns, before quantity types
or broad convenience features.

## Implemented

- Native DuckDB 2.0.0 / C Extension API v2 integration over a pinned UCUM engine.
- Multiplicative compound conversions, dimensional checks, and explicit absolute/delta temperatures.
- Strict and checked conversion, numerical boundary handling, and bounded worker-local caches.
- Strict and checked unit parsing, with auditable NULL/error semantics.
- Strict and checked quantity-text parsing: explicit decimal grammar and separator,
  preserved UCUM spelling, numeric/resource bounds, and no alias or locale guessing.
- UCUM atom catalog and substring search, including unsupported-unit diagnostics.
- SQLLogicTests for columnar inputs, resource/numeric limits, registry consistency, table scans,
  prepared queries, and nested-struct validity.
- Refreshed revision/override metadata on every package build, verified by an E2E rebuild/load
  regression test. Rust CI now also executes SQL and metadata tests.
- Measured performance improvements: constant-unit binding specialization, borrowed cached plans,
  and reduced parser allocation/identity work; reproducible Hyperfine harness and report.

## Before publishing a release

- [x] Establish the project's license: [MIT](../LICENSE).
- [ ] Audit template provenance plus third-party dependency/data notices for release artifacts.
  Dependencies and UCUM data retain their respective license terms.
- [ ] Independently run/version a UCUM conformance corpus against the exposed SQL policy.
  Distinguish deliberately unsupported cases from parser/conversion defects; preserve expected
  tolerances rather than treating a backend's own conformance statement as validation of the wrapper.
- [ ] Add automated adversarial/fuzz coverage for unit strings and verify numerical invariants.
- [ ] Validate release binaries on the supported native platforms and exercise WASM separately.
  A successful host test or a successful WASM compile alone is not cross-platform SQL validation.
- [ ] Produce versioned artifacts and an explicit DuckDB compatibility matrix.
- [ ] Submit the descriptor and required build/test integration to the community registry.

## Next functional improvements

1. **Explicit normalization.** Optional user-controlled aliases (`kWh` → `kW.h`, for example),
   returning original input, resolved code, and the applied rule. Strict parsing stays the default;
   `C` must not silently switch from coulomb to Celsius.
2. **Conversion explanations.** Expose the selected conversion mode and scale/offset, with
   correct treatment of wide numeric coefficients. Never advertise a finite DOUBLE coefficient
   when only the final scaled measurement is representable.
3. **Dataset validation recipes.** Add useful SQL examples for status summaries, quarantine
   tables, and expected-dimension contracts. Prefer composable SQL over a function for every report.
4. **Optional quantity kinds.** Explicit metadata can distinguish energy/torque and similar
   dimensionally identical quantities. Neither annotations nor the catalog's descriptive property
   labels currently enforce such contracts.

## Performance work requires measurements

- Specialization for one constant and one column-driven unit, if measurements justify it.
- Dictionary-aware execution and caching strategies for high-cardinality columns.
- Hyperfine comparisons for constant, low-cardinality, high-cardinality, invalid and NULL inputs,
  with identical outputs, fixed thread counts, pinned revisions, and recorded build settings.

The implementation has per-worker parsing and pair caches, bind-time preparation of constant
source/target conversions, and an ordinary-conversion FMA path. See the [benchmark report](../benchmarks/README.md)
for completed measurements. Further optimizations still require representative comparisons.

## Deliberately deferred

- A custom quantity numeric type and full quantity algebra.
- Uncertainty propagation or automatic unit/quantity-kind inference.
- Logarithmic/reference-dependent or arbitrary-unit conversion without explicit contracts and tests.
- Currency exchange, context-dependent process conversions, or physical-plausibility checks.
