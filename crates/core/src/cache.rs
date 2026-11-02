//! Bounded, query/worker-local caches. Neither user input nor errors live globally.
use std::{collections::HashMap, sync::Arc};
use units_engine::{ConversionPlan, MAX_UNIT_BYTES, ParsedUnit, UnitError};

type Outcome<T> = std::result::Result<T, UnitError>;
const CAPACITY: usize = 256;

#[derive(Default)]
pub struct Cache {
    units: HashMap<String, Outcome<Arc<ParsedUnit>>>,
    // Nested maps allow borrowed &str lookups without allocating per input row.
    plans: HashMap<String, HashMap<String, Outcome<ConversionPlan>>>,
    plan_count: usize,
}

impl Cache {
    pub fn parse(&mut self, unit: &str) -> Outcome<Arc<ParsedUnit>> {
        if let Some(result) = self.units.get(unit) {
            return result.clone();
        }
        let result = units_engine::parse(unit).map(Arc::new);
        if unit.len() <= MAX_UNIT_BYTES {
            if self.units.len() == CAPACITY {
                self.units.clear();
            }
            self.units.insert(unit.to_owned(), result.clone());
        }
        result
    }

    // Each function instance has a fixed delta mode, so it need not be part of the key.
    pub fn convert(&mut self, value: f64, from: &str, to: &str, delta: bool) -> Outcome<f64> {
        // Use the cached plan by reference: copying its wide decimal coefficients
        // for every row is unnecessary and sensitive to compiler inlining.
        let evaluate = |result: &Outcome<ConversionPlan>| {
            result
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|plan| plan.convert(value))
        };
        if let Some(result) = self.plans.get(from).and_then(|targets| targets.get(to)) {
            return evaluate(result);
        }
        let result = self.parse(from).and_then(|a| {
            self.parse(to)
                .and_then(|b| ConversionPlan::new(&a, &b, delta))
        });
        let converted = evaluate(&result);
        if from.len() <= MAX_UNIT_BYTES && to.len() <= MAX_UNIT_BYTES {
            if self.plan_count == CAPACITY {
                self.plans.clear();
                self.plan_count = 0;
            }
            self.plans
                .entry(from.to_owned())
                .or_default()
                .insert(to.to_owned(), result);
            self.plan_count += 1;
        }
        converted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_plans_do_not_cache_value_dependent_errors() {
        let mut cache = Cache::default();
        assert_eq!(
            cache
                .convert(f64::NAN, "km", "m", false)
                .unwrap_err()
                .status,
            "non_finite_input"
        );
        assert_eq!(cache.convert(1.0, "km", "m", false).unwrap(), 1000.0);
        assert_eq!(
            cache
                .convert(f64::MAX, "km", "m", false)
                .unwrap_err()
                .status,
            "overflow"
        );
        assert_eq!(cache.convert(2.0, "km", "m", false).unwrap(), 2000.0);
        assert_eq!(
            cache
                .convert(f64::NAN, "madeup", "m", false)
                .unwrap_err()
                .status,
            "unknown_unit"
        );
    }

    #[test]
    fn cache_is_bounded_including_failures() {
        let mut cache = Cache::default();
        for i in 0..1024 {
            let _ = cache.convert(1.0, &format!("unknown{i}"), "m", false);
            assert!(cache.units.len() <= CAPACITY);
            assert!(cache.plan_count <= CAPACITY);
        }
        assert_eq!(cache.convert(1.0, "km", "m", false).unwrap(), 1000.0);
    }
}
