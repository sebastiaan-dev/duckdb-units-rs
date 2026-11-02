//! Read-only, chunked discovery tables. Search never changes strict parsing semantics.
use std::sync::atomic::{AtomicUsize, Ordering};

use duckdb_neo::{
    DuckDBType, Result,
    bind_arguments::BindArgument,
    connection::Context,
    data_chunk::DataChunkRef,
    error::Error,
    extension::Extension,
    signature::{FunctionParameter, SignatureBuilder},
    table_function::{
        BindFunctionHandle, ExecColumnInfo, InitColumnData, TableFunctionBuilder,
        TableFunctionCallbacks,
    },
};
use units_engine::catalog::{CatalogEntry, catalog, search};

use crate::{docs, sql_error};

#[derive(Clone, Copy)]
enum Column {
    Unit,
    Name,
    Property,
    Prefixable,
    IsBase,
    Supported,
    Status,
    Message,
    Dimension,
    BaseUnit,
    Factor,
    Offset,
    IsAffine,
}

// One shared schema governs both bind-time names/types and projected output positions.
const COLUMNS: [(&str, Column); 13] = [
    ("unit", Column::Unit),
    ("name", Column::Name),
    ("property", Column::Property),
    ("prefixable", Column::Prefixable),
    ("is_base", Column::IsBase),
    ("supported", Column::Supported),
    ("status", Column::Status),
    ("message", Column::Message),
    ("dimension", Column::Dimension),
    ("base_unit", Column::BaseUnit),
    ("factor", Column::Factor),
    ("offset", Column::Offset),
    ("is_affine", Column::IsAffine),
];

struct Catalog {
    search: bool,
}

impl TableFunctionCallbacks for Catalog {
    type BindData = Vec<&'static CatalogEntry>;
    type GlobalState = AtomicUsize;
    type LocalState = ();

    fn bind(
        &self,
        context: &Context,
        arguments: Vec<BindArgument>,
        handle: BindFunctionHandle<'_>,
    ) -> Result<Self::BindData> {
        for (name, column) in COLUMNS {
            let logical_type = match column {
                Column::Prefixable | Column::IsBase | Column::Supported | Column::IsAffine => {
                    bool::logical_type(context)?
                }
                Column::Factor | Column::Offset => f64::logical_type(context)?,
                _ => String::logical_type(context)?,
            };
            handle.add_result_column(name, &logical_type)?;
        }
        let rows = if self.search {
            let value = arguments
                .first()
                .and_then(|a| a.value.as_ref())
                .ok_or_else(|| {
                    Error::invalid_input("unit_search requires a constant search string")
                })?;
            match value.get::<String>()? {
                Some(query) => search(&query).map_err(sql_error)?,
                None => Vec::new(),
            }
        } else {
            catalog().iter().collect()
        };
        handle.set_cardinality(rows.len(), true)?;
        Ok(rows)
    }

    fn init_global_state(
        &self,
        _: &Self::BindData,
        _: &Context,
        _: InitColumnData<'_>,
    ) -> Result<(Option<Self::GlobalState>, Option<usize>)> {
        // The registry is tiny. One scan worker avoids scheduling overhead; the
        // atomic cursor also makes work claims safe if this policy changes later.
        Ok((Some(AtomicUsize::new(0)), Some(1)))
    }

    fn exec(
        &self,
        rows: &Self::BindData,
        state: Option<&Self::GlobalState>,
        _: Option<&mut ()>,
        _: &Context,
        mut output: DataChunkRef<'_>,
        columns: ExecColumnInfo<'_>,
    ) -> Result<()> {
        let cursor = state.ok_or_else(|| Error::internal("units catalog: missing scan state"))?;
        let capacity = output.capacity()?;
        let start = cursor
            .fetch_add(capacity, Ordering::Relaxed)
            .min(rows.len());
        let end = start.saturating_add(capacity).min(rows.len());
        let rows = &rows[start..end];
        for position in 0..columns.count()? {
            let column_index = columns.get_column_index(position)?;
            let (_, column) = COLUMNS
                .get(column_index)
                .ok_or_else(|| Error::internal("units catalog: invalid projected column"))?;
            match column {
                Column::Prefixable | Column::IsBase | Column::Supported | Column::IsAffine => {
                    let mut vector = output.get_vector_at_mut::<bool>(position)?;
                    vector.set_size(rows.len())?;
                    let mut writer = vector.writer()?;
                    for (row, entry) in rows.iter().enumerate() {
                        let value = match column {
                            Column::Prefixable => Some(entry.prefixable),
                            Column::IsBase => Some(entry.is_base),
                            Column::Supported => Some(entry.definition.is_ok()),
                            Column::IsAffine => entry.definition.as_ref().ok().map(|d| d.is_affine),
                            _ => unreachable!(),
                        };
                        writer.set(row, value)?;
                    }
                }
                Column::Factor | Column::Offset => {
                    let mut vector = output.get_vector_at_mut::<f64>(position)?;
                    vector.set_size(rows.len())?;
                    let mut writer = vector.writer()?;
                    for (row, entry) in rows.iter().enumerate() {
                        let value = entry.definition.as_ref().ok().map(|d| match column {
                            Column::Factor => d.factor,
                            _ => d.offset,
                        });
                        writer.set(row, value)?;
                    }
                }
                _ => {
                    let mut vector = output.get_vector_at_mut::<String>(position)?;
                    vector.set_size(rows.len())?;
                    let mut writer = vector.writer()?;
                    for (row, entry) in rows.iter().enumerate() {
                        let value = match column {
                            Column::Unit => Some(entry.unit),
                            Column::Name => Some(entry.name),
                            Column::Property => Some(entry.property),
                            Column::Status => Some(entry.status()),
                            Column::Message => entry.message(),
                            Column::Dimension => {
                                entry.definition.as_ref().ok().map(|d| d.dimension.as_str())
                            }
                            Column::BaseUnit => {
                                entry.definition.as_ref().ok().map(|d| d.base_unit.as_str())
                            }
                            _ => unreachable!(),
                        };
                        writer.set(row, value)?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn register(extension: &Extension) -> Result<()> {
    TableFunctionBuilder::new("units_catalog", SignatureBuilder::new().documentation(docs(
        "List the pinned UCUM atom registry, including unsupported units with diagnostics. Prefixes and compound expressions are not enumerated. Property labels are descriptive, not quantity-kind enforcement.",
        "SELECT unit, name, property, supported FROM units_catalog() ORDER BY unit")), Catalog { search: false })
        .with_projection_pushdown(true).register(extension)?;
    TableFunctionBuilder::new("unit_search", SignatureBuilder::new()
        .param(FunctionParameter::new::<String>("query"))
        .documentation(docs(
            "Case-insensitive substring search of registered unit codes, names and properties. Discovery only, not alias resolution. Empty query lists all atoms; NULL returns no rows. Use ORDER BY for ordering.",
            "SELECT unit, name, supported FROM unit_search('pressure') ORDER BY unit")), Catalog { search: true })
        .with_projection_pushdown(true).register(extension)
}
