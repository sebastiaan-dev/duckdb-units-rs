//! Discovery over the pinned UCUM atom registry, with this extension's support policy.
//! This is a finite vocabulary of atoms, not an enumeration of all unit expressions.
use std::sync::OnceLock;

use crate::{MAX_UNIT_BYTES, ParsedUnit, Result, UnitError, parse};

#[derive(Debug)]
pub struct CatalogEntry {
    pub unit: &'static str,
    pub name: &'static str,
    pub property: &'static str,
    pub prefixable: bool,
    pub is_base: bool,
    pub definition: Result<ParsedUnit>,
}

impl CatalogEntry {
    pub fn status(&self) -> &'static str {
        match &self.definition {
            Ok(_) => "ok",
            Err(error) => error.status,
        }
    }

    pub fn message(&self) -> Option<&str> {
        self.definition.as_ref().err().map(|e| e.message.as_str())
    }
}

/// One row per registered UCUM atom, including unsupported special units.
/// Only immutable library definitions are cached globally, never user expressions.
pub fn catalog() -> &'static [CatalogEntry] {
    static CATALOG: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        octofhir_ucum::get_all_units()
            .iter()
            .map(|unit| CatalogEntry {
                unit: unit.code,
                name: unit.display_name,
                property: unit.property,
                prefixable: unit.is_metric,
                is_base: unit.is_base,
                definition: parse(unit.code),
            })
            .collect()
    })
}

/// Case-insensitive substring search of codes, display names, and UCUM properties.
/// Search is discovery only: it never rewrites a measurement's unit or changes parsing.
pub fn search(query: &str) -> Result<Vec<&'static CatalogEntry>> {
    if query.len() > MAX_UNIT_BYTES {
        return Err(UnitError::new(
            "resource_limit",
            "unit search queries are limited to 4096 bytes",
        ));
    }
    let query = query.to_lowercase();
    Ok(catalog()
        .iter()
        .filter(|entry| {
            [entry.unit, entry.name, entry.property]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConversionPlan;
    use std::collections::HashSet;

    #[test]
    fn every_atom_has_an_auditable_support_decision() {
        let entries = catalog();
        assert!(entries.len() > 250);
        assert_eq!(entries.iter().filter(|u| u.is_base).count(), 7);
        let mut codes = HashSet::new();
        for entry in entries {
            assert!(codes.insert(entry.unit), "duplicate {}", entry.unit);
            assert_eq!(entry.definition.is_ok(), parse(entry.unit).is_ok());
            match &entry.definition {
                Ok(definition) => {
                    assert_eq!(entry.status(), "ok");
                    assert!(entry.message().is_none());
                    assert_eq!(
                        ConversionPlan::new(definition, definition, false)
                            .unwrap()
                            .convert(1.0)
                            .unwrap(),
                        1.0
                    );
                }
                Err(_) => {
                    assert_ne!(entry.status(), "ok");
                    assert!(entry.message().is_some_and(|message| !message.is_empty()));
                }
            }
        }
        let celsius = entries.iter().find(|u| u.unit == "Cel").unwrap();
        assert!(celsius.definition.as_ref().unwrap().is_affine);
        let bel = entries.iter().find(|u| u.unit == "B").unwrap();
        assert_eq!(bel.status(), "unsupported_unit");
    }

    #[test]
    fn searches_names_codes_and_properties_without_aliasing() {
        for query in ["pressure", "PRESSURE", "Pressure"] {
            assert!(search(query).unwrap().iter().any(|unit| unit.unit == "Pa"));
        }
        assert!(
            search("celsius")
                .unwrap()
                .iter()
                .any(|unit| unit.unit == "Cel")
        );
        assert!(
            search("[degF]")
                .unwrap()
                .iter()
                .any(|unit| unit.unit == "[degF]")
        );
        assert_eq!(search("").unwrap().len(), catalog().len());
        assert!(search("not-a-physical-unit").unwrap().is_empty());
        assert!(search("\0").unwrap().is_empty());
        assert_eq!(
            search(&"x".repeat(MAX_UNIT_BYTES + 1)).unwrap_err().status,
            "resource_limit"
        );
        assert!(parse("celsius").is_err());
    }
}
