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
    pub fn plan(&mut self, from: &str, to: &str, delta: bool) -> Outcome<ConversionPlan> {
        if let Some(result) = self.plans.get(from).and_then(|targets| targets.get(to)) {
            return result.clone();
        }
        let result = self.parse(from).and_then(|a| {
            self.parse(to)
                .and_then(|b| ConversionPlan::new(&a, &b, delta))
        });
        if from.len() <= MAX_UNIT_BYTES && to.len() <= MAX_UNIT_BYTES {
            if self.plan_count == CAPACITY {
                self.plans.clear();
                self.plan_count = 0;
            }
            self.plans
                .entry(from.to_owned())
                .or_default()
                .insert(to.to_owned(), result.clone());
            self.plan_count += 1;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_is_bounded_including_failures() {
        let mut cache = Cache::default();
        for i in 0..1024 {
            let _ = cache.plan(&format!("unknown{i}"), "m", false);
            assert!(cache.units.len() <= CAPACITY);
            assert!(cache.plan_count <= CAPACITY);
        }
        assert_eq!(
            cache.plan("km", "m", false).unwrap().convert(1.0).unwrap(),
            1000.0
        );
    }
}
