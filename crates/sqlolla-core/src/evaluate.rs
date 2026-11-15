//! Pure bag evaluation with SQL three-valued Boolean behavior.
use crate::json::error;
use crate::*;

/// Validate every supplied table, including unused tables; absent unused tables are allowed.
/// Referenced absent tables are rejected later by `eval` when their scan is evaluated.
pub fn validate_database(catalog: &Catalog, database: &Database) -> Result<()> {
    for (id, rows) in database {
        let table = catalog
            .tables
            .get(id)
            .ok_or_else(|| error("invalid.database", format!("Unknown table identifier {id}")))?;
        validate(rows, &table.columns)?;
    }
    Ok(())
}

/// Interpret one bound relation without database, network or filesystem effects.
pub fn eval(plan: &Plan, database: &Database) -> Result<Bag> {
    let rows = evaluate_node(plan, database)?;
    validate(&rows, &plan.schema)?;
    Ok(rows)
}

/// Keep the legacy evaluator restricted to its unordered relational contract.
fn evaluate_node(plan: &Plan, database: &Database) -> Result<Bag> {
    Ok(match &plan.node {
        RelNode::Values { rows } => rows.clone(),
        RelNode::EvaluateValues { rows } => evaluate_values(rows)?,
        RelNode::Scan { table } => scan(table, database)?,
        RelNode::Filter { input, predicate } => filter(eval(input, database)?, predicate)?,
        RelNode::Project { input, expressions } => project(eval(input, database)?, expressions)?,
        RelNode::Product { left, right } => {
            product(&eval(left, database)?, &eval(right, database)?)
        }
        RelNode::Join {
            kind,
            left,
            right,
            on,
        } => join(*kind, left, right, on, database)?,
        RelNode::Distinct { input } => unique_rows(eval(input, database)?)?,
        RelNode::Set {
            op,
            all,
            left,
            right,
        } => set(*op, *all, eval(left, database)?, eval(right, database)?)?,
        _ => return ordered_legacy(),
    })
}

/// A missing declared scan cannot masquerade as an empty relation.
fn scan(table: &str, database: &Database) -> Result<Bag> {
    database
        .get(table)
        .cloned()
        .ok_or_else(|| error("invalid.database", format!("Missing table {table}")))
}

/// Evaluate a finite VALUES expression matrix after planning and stable-context substitution.
fn evaluate_values(rows: &[Vec<BoundExpr>]) -> Result<Bag> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|expr| eval_expr(expr, &vec![]))
                .collect::<Result<Row>>()
        })
        .collect()
}

/// Retain the deterministic representative of every SQL duplicate-equivalence class.
fn unique_rows(rows: Bag) -> Result<Bag> {
    Ok(counts(rows)?.into_iter().map(|(row, _)| row).collect())
}

/// Ordering requires the complete statement interpreter rather than the legacy bag API.
fn ordered_legacy() -> Result<Bag> {
    Err(error(
        "unsupported.ordered_legacy",
        "Use complete statement evaluation",
    ))
}

/// Check actual rows against the plan's type, arity and nullability contract.
pub(crate) fn validate(rows: &Bag, schema: &[Column]) -> Result<()> {
    for row in rows {
        if row.len() != schema.len() {
            return Err(error("invalid.row_arity", "Wrong row arity"));
        }
        for (cell, column) in row.iter().zip(schema) {
            cell.validate()?;
            if cell.sql_type() != column.ty || (cell.is_null() && !column.nullable) {
                return Err(error(
                    "invalid.row_type",
                    format!("Invalid cell in {}", column.name),
                ));
            }
        }
    }
    Ok(())
}

/// Evaluate a typed scalar expression; NULL Boolean means SQL UNKNOWN.
pub fn eval_expr(expr: &BoundExpr, row: &Row) -> Result<Cell> {
    let value = match &expr.node {
        ExprNode::Literal(value) => value.clone(),
        ExprNode::Column(index) => row
            .get(*index)
            .cloned()
            .ok_or_else(|| error("invalid.column_index", "Bound column outside row"))?,
        ExprNode::Not(arg) => match truth(eval_expr(arg, row)?)? {
            Some(value) => Cell::Bool(!value),
            None => Cell::Null(SqlType::Bool),
        },
        ExprNode::IsNull { arg, negated } => Cell::Bool(eval_expr(arg, row)?.is_null() != *negated),
        ExprNode::Binary { op, left, right } => {
            binary(*op, eval_expr(left, row)?, eval_expr(right, row)?)?
        }
        _ => crate::scalar_eval::evaluate(expr, row)?,
    };
    if value.sql_type() != expr.ty || (value.is_null() && !expr.nullable) {
        return Err(error(
            "invalid.expression_type",
            "Expression annotation disagrees with value",
        ));
    }
    value.validate()?;
    Ok(value)
}

/// Convert a Boolean scalar to three-valued logic, rejecting other scalar families.
fn truth(value: Cell) -> Result<Option<bool>> {
    match value {
        Cell::Bool(value) => Ok(Some(value)),
        Cell::Null(SqlType::Bool) => Ok(None),
        _ => Err(error("invalid.predicate_type", "Expected SQL Boolean")),
    }
}

/// Apply comparison or Boolean semantics without coercion after binding.
fn binary(op: BinaryOp, left: Cell, right: Cell) -> Result<Cell> {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return logic(op, truth(left)?, truth(right)?);
    }
    crate::comparison::operator(op, &left.sql_type())?;
    let xid = matches!(op, BinaryOp::Eq | BinaryOp::Ne)
        && left.sql_type() == SqlType::Xid
        && right.sql_type() == SqlType::Int32;
    if !xid && !scalar::compatible(&left.sql_type(), &right.sql_type()) {
        return Err(error("invalid.comparison", "Mixed scalar types"));
    }
    if left.is_null() || right.is_null() {
        return Ok(Cell::Null(SqlType::Bool));
    }
    let value = match op {
        BinaryOp::Eq => scalar::compare_sql(&left, &right)? == std::cmp::Ordering::Equal,
        BinaryOp::Ne => scalar::compare_sql(&left, &right)? != std::cmp::Ordering::Equal,
        _ => compare_int(op, left, right)?,
    };
    Ok(Cell::Bool(value))
}

/// Evaluate integer-only ordered comparisons in the supported profile.
fn compare_int(op: BinaryOp, left: Cell, right: Cell) -> Result<bool> {
    crate::comparison::ordered(&left.sql_type())?;
    let order = scalar::compare_sql(&left, &right)?;
    match op {
        BinaryOp::Lt => Ok(order.is_lt()),
        BinaryOp::Le => Ok(!order.is_gt()),
        BinaryOp::Gt => Ok(order.is_gt()),
        BinaryOp::Ge => Ok(!order.is_lt()),
        _ => Err(error("invalid.operator", "Invalid comparison operator")),
    }
}

/// FALSE dominates AND and TRUE dominates OR; otherwise preserve UNKNOWN.
fn logic(op: BinaryOp, left: Option<bool>, right: Option<bool>) -> Result<Cell> {
    let dominating = matches!(op, BinaryOp::Or);
    if left == Some(dominating) || right == Some(dominating) {
        return Ok(Cell::Bool(dominating));
    }
    if left.is_none() || right.is_none() {
        return Ok(Cell::Null(SqlType::Bool));
    }
    Ok(Cell::Bool(!dominating))
}

/// Retain exactly TRUE predicate results, including duplicate row occurrences.
fn filter(rows: Bag, predicate: &BoundExpr) -> Result<Bag> {
    let mut result = Vec::new();
    for row in rows {
        if truth(eval_expr(predicate, &row)?)? == Some(true) {
            result.push(row);
        }
    }
    Ok(result)
}

/// Project each occurrence independently, preserving projection collisions.
fn project(rows: Bag, expressions: &[BoundExpr]) -> Result<Bag> {
    rows.iter()
        .map(|row| {
            expressions
                .iter()
                .map(|expr| eval_expr(expr, row))
                .collect()
        })
        .collect()
}

/// Construct one distinct occurrence for every input occurrence pair.
fn product(left: &Bag, right: &Bag) -> Bag {
    left.iter()
        .flat_map(|a| right.iter().map(move |b| combine(a, b)))
        .collect()
}

/// Concatenate positional cells without changing either input row.
fn combine(left: &Row, right: &Row) -> Row {
    left.iter().chain(right).cloned().collect()
}

/// Create a typed null-extension row for one side of an outer join.
fn null_row(schema: &[Column]) -> Row {
    schema
        .iter()
        .map(|column| Cell::Null(column.ty.clone()))
        .collect()
}

/// Join each occurrence pair and extend exactly the unmatched occurrences.
fn join(kind: JoinKind, left: &Plan, right: &Plan, on: &BoundExpr, db: &Database) -> Result<Bag> {
    let (a, b) = (eval(left, db)?, eval(right, db)?);
    let mut result = Vec::new();
    let mut right_matched = vec![false; b.len()];
    for row in &a {
        let mut matched = false;
        for (index, other) in b.iter().enumerate() {
            let combined = combine(row, other);
            if truth(eval_expr(on, &combined)?)? == Some(true) {
                result.push(combined);
                matched = true;
                right_matched[index] = true;
            }
        }
        if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
            result.push(combine(row, &null_row(&right.schema)));
        }
    }
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for (index, row) in b.iter().enumerate() {
            if !right_matched[index] {
                result.push(combine(&null_row(&left.schema), row));
            }
        }
    }
    Ok(result)
}

/// Count whole-row duplicate equivalence, including equal typed NULLs.
fn counts(rows: Bag) -> Result<Vec<(Row, usize)>> {
    let mut result: Vec<(Row, usize)> = Vec::new();
    for row in rows {
        let mut found = None;
        for (index, (other, _)) in result.iter().enumerate() {
            if sql_row_equal(&row, other)? {
                found = Some(index);
                break;
            }
        }
        if let Some(index) = found {
            result[index].1 += 1;
        } else {
            result.push((row, 1));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Implement ALL and DISTINeT variants using SQL row multiplicities.
fn set(op: SetOp, all: bool, left: Bag, right: Bag) -> Result<Bag> {
    let keys = counts(left.iter().chain(&right).cloned().collect())?;
    let (a, b) = (counts(left)?, counts(right)?);
    let mut result = Vec::new();
    for (row, _) in keys {
        let (x, y) = (multiplicity(&row, &a)?, multiplicity(&row, &b)?);
        let amount = match op {
            SetOp::Union => x + y,
            SetOp::Intersect => x.min(y),
            SetOp::Except if all => x.saturating_sub(y),
            SetOp::Except => usize::from(x > 0 && y == 0),
        };
        let count = if all { amount } else { usize::from(amount > 0) };
        result.extend(std::iter::repeat_n(row.clone(), count));
    }
    Ok(result)
}

/// Locate a SQL duplicate-equivalence class independently of transport representation.
fn multiplicity(row: &Row, counts: &[(Row, usize)]) -> Result<usize> {
    for (other, count) in counts {
        if sql_row_equal(row, other)? {
            return Ok(*count);
        }
    }
    Ok(0)
}

/// Compare positional rows by SQL duplicate equivalence, including typed NULL values.
pub fn sql_row_equal(left: &Row, right: &Row) -> Result<bool> {
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

/// Compare unordered occurrence bags without collapsing duplicate rows or numeric representations.
pub fn sql_bag_equal(left: &Bag, right: &Bag) -> Result<bool> {
    if left.len() != right.len() {
        return Ok(false);
    }
    let a = counts(left.clone())?;
    let b = counts(right.clone())?;
    for (row, count) in a {
        if multiplicity(&row, &b)? != count {
            return Ok(false);
        }
    }
    Ok(true)
}
