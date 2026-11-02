//! Strict and checked parsing of a textual measurement and its UCUM unit.
use duckdb_neo::{
    AsDuckDB, DuckDBType, Result,
    connection::Context,
    data_chunk::VectorCollection,
    enums::{FunctionProperty, FunctionPropertyCollation},
    extension::Extension,
    scalar::{ScalarCallbacks, ScalarFunctionBuilder, ScalarInitCallbacks},
    signature::{FunctionParameter, SignatureBuilder},
    vector::{Unknown, Vector},
};
use units_engine::UnitError;

use crate::{UnitInfo, cache::Cache, docs, sql_error, state};

#[derive(Debug, AsDuckDB)]
struct QuantityInfo {
    value: f64,
    unit: String,
    definition: UnitInfo,
}

#[derive(Debug, AsDuckDB)]
struct CheckedQuantity {
    quantity: Option<QuantityInfo>,
    status: String,
    message: Option<String>,
}

impl CheckedQuantity {
    fn from_result(result: std::result::Result<QuantityInfo, UnitError>) -> Self {
        match result {
            Ok(quantity) => Self {
                quantity: Some(quantity),
                status: "ok".into(),
                message: None,
            },
            Err(error) => Self {
                quantity: None,
                status: error.status.into(),
                message: Some(error.message),
            },
        }
    }
}

fn parse(text: &str, cache: &mut Cache) -> std::result::Result<QuantityInfo, UnitError> {
    let parts = units_engine::quantity::split(text)?;
    // Cache units, not entire quantities: measurement values usually differ row to row.
    let definition = cache.parse(parts.unit)?;
    Ok(QuantityInfo {
        value: parts.value,
        unit: parts.unit.to_owned(),
        definition: UnitInfo::from(definition.as_ref()),
    })
}

struct QuantityParser {
    checked: bool,
}

impl ScalarInitCallbacks for QuantityParser {
    fn init(&self, _: Option<&()>, _: &Context) -> Result<Cache> {
        Ok(Cache::default())
    }
}

impl ScalarCallbacks for QuantityParser {
    type BindData = ();
    type InitData = Cache;

    fn exec(
        &self,
        _: Option<&()>,
        init: Option<&mut Cache>,
        _: &Context,
        vectors: &mut VectorCollection,
        output: Vector<'_, Unknown>,
    ) -> Result<()> {
        let cache = state(init)?;
        let rows = vectors.row_count()?;
        let input = vectors.get_vector_at::<String>(0)?.reader()?;
        if self.checked {
            let mut output = output.cast::<CheckedQuantity>()?;
            output.set_size(rows)?;
            let mut writer = output.writer()?;
            for (row, text) in input.iter().enumerate() {
                let result = match text {
                    Some(text) => parse(text, cache),
                    None => Err(UnitError::new(
                        "missing_input",
                        "quantity text must be present",
                    )),
                };
                writer.set(row, Some(CheckedQuantity::from_result(result)))?;
            }
        } else {
            let mut output = output.cast::<QuantityInfo>()?;
            output.set_size(rows)?;
            let mut writer = output.writer()?;
            for (row, text) in input.iter().enumerate() {
                let result = text
                    .map(|text| parse(text, cache))
                    .transpose()
                    .map_err(sql_error)?;
                writer.set(row, result)?;
            }
        }
        Ok(())
    }
}

pub fn register(extension: &Extension) -> Result<()> {
    for (name, checked, description, example) in [
        (
            "quantity_parse",
            false,
            "Parse a decimal value and supported UCUM unit, separated by a space or tab. Returns STRUCT(value, unit, definition). No aliases or locale guessing. NULL stays NULL; invalid input raises an error.",
            "SELECT quantity_parse('12.5 kg/m3')",
        ),
        (
            "quantity_parse_checked",
            true,
            "Checked quantity parsing: returns STRUCT(quantity, status, message). Invalid input has NULL quantity; NULL text has status missing_input. A space or tab must separate number and unit.",
            "SELECT quantity_parse_checked('12.5 kg/m3')",
        ),
    ] {
        let result_type = if checked {
            CheckedQuantity::logical_type(extension)?
        } else {
            QuantityInfo::logical_type(extension)?
        };
        ScalarFunctionBuilder::new(
            name,
            SignatureBuilder::new()
                .param(FunctionParameter::new::<String>("text"))
                .return_type(result_type)
                .documentation(docs(description, example)),
            QuantityParser { checked },
        )
        .with_init()
        .set_property(FunctionProperty::HasSpecialNullHandling(checked))
        .set_property(FunctionProperty::CollationHandling(
            FunctionPropertyCollation::Ignore,
        ))
        .register(extension)?;
    }
    Ok(())
}
