//! Explicit immutable SQL function definitions are parsed and expanded without external execution.
use crate::json::error;
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Declared function lookup context for the admitted immutable SQL function profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlFunctions {
    /// Effective schema search path, required to agree with the enclosing catalog.
    pub search_path: Vec<String>,
    /// One supported definition per schema/name; overload ambiguity is not guessed.
    pub definitions: Vec<ImmutableSqlFunction>,
}

/// Scalar LANGUAGE SQL IMMUTABLE, non-STRICT, SECURITY INVOKER function without SET/defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImmutableSqlFunction {
    /// Explicit attestation of the supported declaration profile.
    pub profile: String,
    /// Function schema identity.
    pub schema: String,
    /// Function name, independent of the SQL body text.
    pub name: String,
    /// EXECUTE permission for the explicitly modeled role.
    pub can_execute: bool,
    /// Schema USAGE permission for explicitly qualified resolution.
    pub can_use_schema: bool,
    /// Named typed parameter positions; unnamed parameters use stable generated names.
    pub parameters: Vec<Column>,
    /// Declared scalar return type.
    pub returns: SqlType,
    /// Exact scalar SELECT body, reparsed by the pinned adapter.
    pub body_sql: String,
    /// Internal parser evidence is always regenerated from body_sql.
    #[serde(skip)]
    pub prepared: Option<serde_json::Value>,
}

/// Validate declared identities and budgets before any function body can be expanded.
pub(crate) fn validate(functions: &SqlFunctions) -> Result<()> {
    if functions.definitions.len() > 64 {
        return Err(error("resource.sql_functions", "Function count exceeds 64"));
    }
    let mut names = BTreeSet::new();
    for function in &functions.definitions {
        if !names.insert((&function.schema, &function.name)) {
            return Err(error(
                "unsupported.function_overloads",
                "Duplicate function name",
            ));
        }
        validate_definition(function)?;
    }
    Ok(())
}

/// Reject definitions outside the explicit profile instead of silently discarding declaration effects.
fn validate_definition(function: &ImmutableSqlFunction) -> Result<()> {
    if function.profile != "sql_immutable_non_strict_invoker_v1" || function.schema == "pg_catalog"
    {
        return Err(error(
            "unsupported.sql_function_profile",
            "Unsupported function declaration profile",
        ));
    }
    crate::value::validate_text(&function.schema)?;
    crate::value::validate_text(&function.name)?;
    if function.parameters.len() > 32
        || function.body_sql.is_empty()
        || function.body_sql.len() > 65_536
    {
        return Err(error(
            "resource.sql_function",
            "Function parameter/source budget exceeded",
        ));
    }
    validate_parameters(&function.parameters)
}

/// Formal parameter names are local SQL identifiers and cannot alias a source relation.
fn validate_parameters(parameters: &[Column]) -> Result<()> {
    let mut names = BTreeSet::new();
    for parameter in parameters {
        crate::value::validate_text(&parameter.name)?;
        if !names.insert(&parameter.name)
            || parameter.qualifier.is_some()
            || parameter.schema.is_some()
        {
            return Err(error(
                "invalid.sql_function_parameter",
                "Parameter names must be unique and unqualified",
            ));
        }
    }
    Ok(())
}

/// Validate lexical lookup before query binding, including reads that never execute a function.
pub(crate) fn validate_catalog(catalog: &Catalog) -> Result<()> {
    if let Some(functions) = &catalog.functions {
        validate(functions)?;
        if functions.search_path != catalog.search_path {
            return Err(error(
                "invalid.sql_functions",
                "Function lookup path contradicts catalog",
            ));
        }
    }
    Ok(())
}

/// Merge identical declaration snapshots without accepting a conflicting execution-only definition.
pub(crate) fn merge(catalog: &Catalog, context: &mut ExecutionContext) -> Result<()> {
    if let Some(functions) = &catalog.functions {
        validate(functions)?;
        if functions.search_path != catalog.search_path
            || context
                .functions
                .as_ref()
                .is_some_and(|other| other != functions)
        {
            return Err(error(
                "invalid.sql_functions",
                "Function lookup context contradicts catalog",
            ));
        }
        context.functions = Some(functions.clone());
    } else if context.functions.is_some() {
        return Err(error(
            "invalid.sql_functions",
            "Function definitions require catalog identity",
        ));
    }
    Ok(())
}
