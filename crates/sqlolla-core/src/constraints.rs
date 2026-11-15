//! Enforced constraints yield SQL errors rather than being assumed as poststate facts.
use crate::json::error;
use crate::*;

/// Initially deferred checks need a transaction ending boundary, absent from statement execution.
pub(crate) fn require_statement_timing(context: &ExecutionContext) -> Result<()> {
    let deferred = context.table_metadata.values().any(|metadata| {
        metadata
            .primary_key
            .iter()
            .chain(&metadata.unique)
            .any(|key| key.initially_deferred)
            || metadata
                .foreign_keys
                .iter()
                .any(|key| key.initially_deferred)
    });
    if deferred {
        return Err(error(
            "unsupported.deferred_constraints",
            "Initially deferred checks require an explicit transaction boundary",
        ));
    }
    Ok(())
}

/// Check declared state constraints; prestate callers classify violations as inadmissibility.
pub(crate) fn database(database: &Database, context: &ExecutionContext) -> Result<()> {
    for (table, metadata) in &context.table_metadata {
        let rows = database
            .get(table)
            .ok_or_else(|| error("invalid.database", "Constraint table is missing"))?;
        for key in metadata.primary_key.iter() {
            unique(rows, key, true)?;
        }
        for key in &metadata.unique {
            unique(rows, key, false)?;
        }
        for key in &metadata.foreign_keys {
            foreign(rows, key, database)?;
        }
    }
    Ok(())
}

/// CHECK rejects FALSE but permits UNKNOWN; expression errors remain explicit SQL failures.
pub(crate) fn checks(
    rows: &Bag,
    metadata: &TableSemantics,
    columns: &[Column],
    context: &ExecutionContext,
) -> Result<()> {
    for check in &metadata.checks {
        let parser = check
            .parser
            .as_ref()
            .ok_or_else(|| error("unsupported.check_binding", "CHECK parser evidence missing"))?;
        let expression = bind_metadata_functions(parser, columns, context.functions.as_ref())?;
        if expression.ty != SqlType::Bool {
            return Err(error("sql.42804", "CHECK expression must be Boolean"));
        }
        for row in rows {
            if crate::execution::session::evaluate(&expression, row, context)? == Cell::Bool(false)
            {
                return Err(violation("23514", Some(&check.name), None));
            }
        }
    }
    Ok(())
}

/// Nondeferrable uniqueness is checked after each row change, before a later row can repair it.
pub(crate) fn immediate_unique(
    rows: &Bag,
    metadata: &TableSemantics,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<()> {
    crate::constraint_order::immediate(rows, metadata, schema, context)
}

/// Preserve SQLSTATE and distinct named-constraint/column diagnostic fields.
pub(crate) fn violation(code: &str, constraint: Option<&str>, column: Option<&str>) -> Error {
    let sql = SqlFailure {
        sqlstate: code.into(),
        phase: FailurePhase::Statement,
        constraint_name: constraint.map(str::to_owned),
        column_name: column.map(str::to_owned),
        exposed_rows: Vec::new(),
        effects: Vec::new(),
    };
    let mut failure = error(
        &format!("sql.{code}"),
        "Declared constraint rejected the candidate",
    );
    failure.sql = Some(Box::new(sql));
    failure
}

/// Compare every occurrence pair using SQL value equality and the declared unique-NULL mode.
pub(crate) fn unique(rows: &Bag, key: &KeyConstraint, primary: bool) -> Result<()> {
    for (index, row) in rows.iter().enumerate() {
        let values = project(row, &key.columns)?;
        if values.iter().any(|value| value.is_null()) {
            if primary {
                return Err(violation("23502", None, None));
            }
            if !key.nulls_not_distinct {
                continue;
            }
        }
        for other in &rows[..index] {
            let other = project(other, &key.columns)?;
            if !key.nulls_not_distinct && other.iter().any(|value| value.is_null()) {
                continue;
            }
            if equal(&values, &other)? {
                return Err(violation("23505", Some(&key.name), None));
            }
        }
    }
    Ok(())
}

/// MATCH SIMPLE permits a referencing key with any NULL component.
fn foreign(rows: &Bag, key: &ForeignKey, database: &Database) -> Result<()> {
    let referenced = database
        .get(&key.referenced_table)
        .ok_or_else(|| error("invalid.database", "Referenced table is missing"))?;
    for row in rows {
        let values = project(row, &key.columns)?;
        if values.iter().any(|value| value.is_null()) {
            continue;
        }
        let mut found = false;
        for other in referenced {
            if equal(&values, &project(other, &key.referenced_columns)?)? {
                found = true;
                break;
            }
        }
        if !found {
            return Err(violation("23503", Some(&key.name), None));
        }
    }
    Ok(())
}

/// Constraint metadata never permits unchecked indexing into a row.
fn project<'a>(row: &'a Row, columns: &[usize]) -> Result<Vec<&'a Cell>> {
    columns
        .iter()
        .map(|index| {
            row.get(*index)
                .ok_or_else(|| error("invalid.constraint_columns", "Column outside row"))
        })
        .collect()
}

/// Rich numeric/display representations use SQL duplicate equivalence, not transport identity.
fn equal(left: &[&Cell], right: &[&Cell]) -> Result<bool> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (a, b) in left.iter().zip(right) {
        if !scalar::duplicate_eq(a, b)? {
            return Ok(false);
        }
    }
    Ok(true)
}
