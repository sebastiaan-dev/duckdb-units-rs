//! Native DuckDB C API v2 integration for the `units` extension.
#![forbid(unsafe_code)]

mod cache;
mod catalog;

use cache::Cache;
use duckdb_neo::{
    AsDuckDB, DuckDBType, Result,
    connection::Context,
    data_chunk::VectorCollection,
    enums::{FunctionProperty, FunctionPropertyCollation},
    error::Error,
    extension::Extension,
    scalar::{ScalarCallbacks, ScalarFunctionBuilder, ScalarInitCallbacks},
    signature::{FunctionDocumentationBuilder, FunctionParameter, SignatureBuilder},
    vector::{Unknown, Vector},
};
use units_engine::UnitError;

fn sql_error(error: UnitError) -> Error {
    Error::invalid_input(format!("units: {error}"))
}
fn state(cache: Option<&mut Cache>) -> Result<&mut Cache> {
    cache.ok_or_else(|| Error::internal("units: missing per-worker cache"))
}
fn docs(description: &str, example: &str) -> FunctionDocumentationBuilder {
    FunctionDocumentationBuilder::new()
        .description(description)
        .add_example("Example", example)
        .add_category("units")
}

#[derive(Debug, AsDuckDB)]
struct CheckedConversion {
    value: Option<f64>,
    status: String,
    message: Option<String>,
}
impl CheckedConversion {
    fn from_result(result: std::result::Result<f64, UnitError>) -> Self {
        match result {
            Ok(value) => Self {
                value: Some(value),
                status: "ok".into(),
                message: None,
            },
            Err(error) => Self {
                value: None,
                status: error.status.into(),
                message: Some(error.message),
            },
        }
    }
}

struct Convert {
    delta: bool,
    checked: bool,
}
impl ScalarInitCallbacks for Convert {
    fn init(&self, _: Option<&()>, _: &Context) -> Result<Cache> {
        Ok(Cache::default())
    }
}
impl ScalarCallbacks for Convert {
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
        let values = vectors.get_vector_at::<f64>(0)?.reader()?;
        let sources = vectors.get_vector_at::<String>(1)?.reader()?;
        let targets = vectors.get_vector_at::<String>(2)?.reader()?;
        let evaluate = |cache: &mut Cache,
                        value: Option<&f64>,
                        from: Option<&str>,
                        to: Option<&str>| match (value, from, to) {
            (Some(v), Some(a), Some(b)) => cache
                .plan(a, b, self.delta)
                .and_then(|plan| plan.convert(*v)),
            _ => Err(UnitError::new(
                "missing_input",
                "value, source unit, and target unit must all be present",
            )),
        };
        if self.checked {
            let mut output = output.cast::<CheckedConversion>()?;
            output.set_size(vectors.row_count()?)?;
            let mut writer = output.writer()?;
            for row in 0..values.len() {
                let result = evaluate(
                    cache,
                    values.get(row)?,
                    sources.get(row)?,
                    targets.get(row)?,
                );
                writer.set(row, Some(CheckedConversion::from_result(result)))?;
            }
        } else {
            let mut output = output.cast::<f64>()?;
            output.set_size(vectors.row_count()?)?;
            let mut writer = output.writer()?;
            for row in 0..values.len() {
                let (value, from, to) = (values.get(row)?, sources.get(row)?, targets.get(row)?);
                if value.is_none() || from.is_none() || to.is_none() {
                    writer.set(row, None)?;
                } else {
                    writer.set(
                        row,
                        Some(evaluate(cache, value, from, to).map_err(sql_error)?),
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, AsDuckDB)]
struct UnitInfo {
    dimension: String,
    base_unit: String,
    factor: f64,
    offset: f64,
    is_affine: bool,
}

impl From<&units_engine::ParsedUnit> for UnitInfo {
    fn from(parsed: &units_engine::ParsedUnit) -> Self {
        Self {
            dimension: parsed.dimension.clone(),
            base_unit: parsed.base_unit.clone(),
            factor: parsed.factor,
            offset: parsed.offset,
            is_affine: parsed.is_affine,
        }
    }
}

#[derive(Debug, AsDuckDB)]
struct CheckedParse {
    definition: Option<UnitInfo>,
    status: String,
    message: Option<String>,
}

impl CheckedParse {
    fn from_result(result: std::result::Result<UnitInfo, UnitError>) -> Self {
        match result {
            Ok(definition) => Self {
                definition: Some(definition),
                status: "ok".into(),
                message: None,
            },
            Err(error) => Self {
                definition: None,
                status: error.status.into(),
                message: Some(error.message),
            },
        }
    }
}

#[derive(Clone, Copy)]
enum Inspect {
    Parse,
    ParseChecked,
    Dimension,
    Valid,
    Backend,
}
impl ScalarInitCallbacks for Inspect {
    fn init(&self, _: Option<&()>, _: &Context) -> Result<Cache> {
        Ok(Cache::default())
    }
}
impl ScalarCallbacks for Inspect {
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
        if matches!(self, Self::Backend) {
            let mut output = output.cast::<String>()?;
            output.set_size(rows)?;
            let mut writer = output.writer()?;
            for row in 0..rows {
                writer.set(row, Some(units_engine::BACKEND))?;
            }
            return Ok(());
        }
        let input = vectors.get_vector_at::<String>(0)?.reader()?;
        match self {
            Self::Valid => {
                let mut output = output.cast::<bool>()?;
                output.set_size(rows)?;
                let mut writer = output.writer()?;
                for (row, unit) in input.iter().enumerate() {
                    writer.set(row, unit.map(|unit| cache.parse(unit).is_ok()))?;
                }
            }
            Self::Dimension => {
                let mut output = output.cast::<String>()?;
                output.set_size(rows)?;
                let mut writer = output.writer()?;
                for (row, unit) in input.iter().enumerate() {
                    let parsed = unit
                        .map(|unit| cache.parse(unit))
                        .transpose()
                        .map_err(sql_error)?;
                    writer.set(row, parsed.as_ref().map(|parsed| parsed.dimension.as_str()))?;
                }
            }
            Self::Parse => {
                let mut output = output.cast::<UnitInfo>()?;
                output.set_size(rows)?;
                let mut writer = output.writer()?;
                for (row, unit) in input.iter().enumerate() {
                    let parsed = unit
                        .map(|unit| cache.parse(unit))
                        .transpose()
                        .map_err(sql_error)?;
                    writer.set(row, parsed.map(|p| UnitInfo::from(p.as_ref())))?;
                }
            }
            Self::ParseChecked => {
                let mut output = output.cast::<CheckedParse>()?;
                output.set_size(rows)?;
                let mut writer = output.writer()?;
                for (row, unit) in input.iter().enumerate() {
                    let result = match unit {
                        Some(unit) => cache.parse(unit).map(|p| UnitInfo::from(p.as_ref())),
                        None => Err(UnitError::new("missing_input", "unit must be present")),
                    };
                    writer.set(row, Some(CheckedParse::from_result(result)))?;
                }
            }
            Self::Backend => unreachable!(),
        }
        Ok(())
    }
}

struct Compatible;
impl ScalarInitCallbacks for Compatible {
    fn init(&self, _: Option<&()>, _: &Context) -> Result<Cache> {
        Ok(Cache::default())
    }
}
impl ScalarCallbacks for Compatible {
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
        let a = vectors.get_vector_at::<String>(0)?.reader()?;
        let b = vectors.get_vector_at::<String>(1)?.reader()?;
        let mut output = output.cast::<bool>()?;
        output.set_size(vectors.row_count()?)?;
        let mut writer = output.writer()?;
        for row in 0..a.len() {
            let result = match (a.get(row)?, b.get(row)?) {
                (Some(a), Some(b)) => Some(units_engine::compatible(
                    cache.parse(a).map_err(sql_error)?.as_ref(),
                    cache.parse(b).map_err(sql_error)?.as_ref(),
                )),
                _ => None,
            };
            writer.set(row, result)?;
        }
        Ok(())
    }
}

pub fn register(extension: &Extension) -> Result<()> {
    catalog::register(extension)?;
    for (name, delta, checked, description, example) in [
        (
            "unit_convert",
            false,
            false,
            "Convert between compatible UCUM units. Invalid units and incompatible dimensions raise errors.",
            "SELECT unit_convert(1, 'kW.h', 'J')",
        ),
        (
            "unit_convert_delta",
            true,
            false,
            "Convert temperature differences without affine offsets. Only temperature dimensions are accepted.",
            "SELECT unit_convert_delta(10, 'Cel', '[degF]')",
        ),
        (
            "unit_convert_checked",
            false,
            true,
            "Convert without aborting on data errors: returns STRUCT(value, status, message). NULL inputs have status missing_input.",
            "SELECT unit_convert_checked(1, 'kg', 'm')",
        ),
        (
            "unit_convert_delta_checked",
            true,
            true,
            "Checked temperature-difference conversion, returning STRUCT(value, status, message).",
            "SELECT unit_convert_delta_checked(10, 'Cel', 'K')",
        ),
    ] {
        let result_type = if checked {
            CheckedConversion::logical_type(extension)?
        } else {
            f64::logical_type(extension)?
        };
        ScalarFunctionBuilder::new(
            name,
            SignatureBuilder::new()
                .param(FunctionParameter::new::<f64>("value"))
                .param(FunctionParameter::new::<String>("source_unit"))
                .param(FunctionParameter::new::<String>("target_unit"))
                .return_type(result_type)
                .documentation(docs(description, example)),
            Convert { delta, checked },
        )
        .with_init()
        .set_property(FunctionProperty::HasSpecialNullHandling(checked))
        .set_property(FunctionProperty::CollationHandling(
            FunctionPropertyCollation::Ignore,
        ))
        .register(extension)?;
    }
    for (name, op, description, example) in [
        (
            "unit_parse",
            Inspect::Parse,
            "Parse a supported UCUM unit: dimension, base_unit, factor, offset, is_affine. Base units follow UCUM, including grams for mass.",
            "SELECT unit_parse('kg/m3')",
        ),
        (
            "unit_parse_checked",
            Inspect::ParseChecked,
            "Parse without aborting on data errors: returns STRUCT(definition, status, message), with the same definition as unit_parse. NULL input has status missing_input.",
            "SELECT unit_parse_checked('Cel/s')",
        ),
        (
            "unit_dimension",
            Inspect::Dimension,
            "Return the UCUM dimension vector as text; does not infer semantic quantity kinds.",
            "SELECT unit_dimension('Pa')",
        ),
        (
            "unit_valid",
            Inspect::Valid,
            "Whether an expression is supported by this extension. Case-sensitive UCUM; unsupported special units return false. NULL stays NULL.",
            "SELECT unit_valid('kg/m3')",
        ),
        (
            "units_backend",
            Inspect::Backend,
            "Return the pinned engine, standard, and policy versions.",
            "SELECT units_backend()",
        ),
    ] {
        let result_type = match op {
            Inspect::Parse => UnitInfo::logical_type(extension)?,
            Inspect::ParseChecked => CheckedParse::logical_type(extension)?,
            Inspect::Valid => bool::logical_type(extension)?,
            _ => String::logical_type(extension)?,
        };
        let mut signature = SignatureBuilder::new()
            .return_type(result_type)
            .documentation(docs(description, example));
        if !matches!(op, Inspect::Backend) {
            signature = signature.param(FunctionParameter::new::<String>("unit"));
        }
        ScalarFunctionBuilder::new(name, signature, op)
            .with_init()
            .set_property(FunctionProperty::HasSpecialNullHandling(matches!(
                op,
                Inspect::ParseChecked
            )))
            .set_property(FunctionProperty::CollationHandling(
                FunctionPropertyCollation::Ignore,
            ))
            .register(extension)?;
    }
    ScalarFunctionBuilder::new("unit_compatible",SignatureBuilder::new()
        .param(FunctionParameter::new::<String>("source_unit"))
        .param(FunctionParameter::new::<String>("target_unit"))
        .return_type(bool::logical_type(extension)?)
        .documentation(docs("Check dimensional compatibility of supported UCUM units. Unknown/unsupported units raise errors; this is not quantity-kind validation.","SELECT unit_compatible('Pa', 'kg/(m.s2)')")),Compatible)
        .with_init().set_property(FunctionProperty::CollationHandling(FunctionPropertyCollation::Ignore))
        .register(extension)
}
