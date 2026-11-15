//! Complete bounded grouping over SQL duplicate classes and exact aggregate values.
use super::*;
use crate::scalar::{arithmetic, cast, compare_sql, duplicate_eq, input, output};

/// One SQL key class retains every occurrence under an unambiguous representation.
struct Group {
    key: Row,
    rows: Bag,
}

/// Lift grouping over every child alternative without treating SQL failures as empty bags.
pub(super) fn evaluate(
    _plan: &Plan,
    input: &Plan,
    groups: &[BoundExpr],
    aggregates: &[AggregateExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut outcomes = Vec::new();
    for relation in super::relational::query(input, db, context)? {
        let values = super::rel_effects::transform(relation, context, |value, context| {
            let rows = grouped(&value.rows, groups, aggregates, db, context)?;
            Ok(vec![result(rows, ResultOrdering::Bag)])
        })?;
        for value in values {
            push(&mut outcomes, value, context)?;
        }
    }
    Ok(outcomes)
}

/// Assemble one row per SQL key class, retaining a global empty-input group.
fn grouped(
    rows: &Bag,
    keys: &[BoundExpr],
    calls: &[AggregateExpr],
    db: &Database,
    ctx: &Runtime<'_>,
) -> Result<Bag> {
    let groups = partition(rows, keys, db, ctx)?;
    let mut bag = Vec::new();
    for group in groups {
        bag.push(group_row(&group, calls, db, ctx)?);
        check_rows(bag.len(), ctx)?;
    }
    Ok(bag)
}

/// Partition by SQL equality, which equates NULL keys and ignores numeric display scale.
fn partition(
    rows: &Bag,
    keys: &[BoundExpr],
    db: &Database,
    ctx: &Runtime<'_>,
) -> Result<Vec<Group>> {
    let mut groups = Vec::<Group>::new();
    if keys.is_empty() {
        return Ok(vec![Group {
            key: vec![],
            rows: rows.clone(),
        }]);
    }
    for row in rows {
        let key = key_values(keys, row, db, ctx)?;
        let index = group_index(&groups, &key)?;
        match index {
            Some(index) => {
                representation(&groups[index].key, &key)?;
                groups[index].rows.push(row.clone());
            }
            None => groups.push(Group {
                key,
                rows: vec![row.clone()],
            }),
        }
    }
    Ok(groups)
}

/// Evaluate grouping keys through the shared context/correlation-aware scalar callback.
fn key_values(keys: &[BoundExpr], row: &Row, db: &Database, ctx: &Runtime<'_>) -> Result<Row> {
    keys.iter()
        .map(|expr| super::scalar(expr, row, db, ctx))
        .collect()
}

/// Find an existing key class without using transport equality as SQL equality.
fn group_index(groups: &[Group], key: &Row) -> Result<Option<usize>> {
    for (index, group) in groups.iter().enumerate() {
        let mut equal = true;
        for (left, right) in group.key.iter().zip(key) {
            equal &= duplicate_eq(left, right)?;
        }
        if equal {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// Append each deterministic aggregate value to the group's validated representative.
fn group_row(
    group: &Group,
    calls: &[AggregateExpr],
    db: &Database,
    ctx: &Runtime<'_>,
) -> Result<Row> {
    let mut row = group.key.clone();
    for call in calls {
        row.push(aggregate(call, &group.rows, db, ctx)?);
    }
    Ok(row)
}

/// FILTER excludes FALSE/UNKNOWN before arguments, and NULL arguments never contribute.
fn arguments(
    call: &AggregateExpr,
    rows: &Bag,
    db: &Database,
    ctx: &Runtime<'_>,
) -> Result<Vec<Cell>> {
    let mut values = Vec::new();
    for row in rows {
        if let Some(filter) = &call.filter {
            if super::scalar(filter, row, db, ctx)? != Cell::Bool(true) {
                continue;
            }
        }
        let value = match &call.argument {
            Some(argument) => super::scalar(argument, row, db, ctx)?,
            None => Cell::Bool(true),
        };
        if !value.is_null() {
            values.push(value);
        }
    }
    Ok(values)
}

/// DISTINCT requires representationally identical members until correlated tie rules exist.
fn distinct_inputs(values: &[Cell]) -> Result<Vec<Cell>> {
    let mut unique = Vec::<Cell>::new();
    for value in values {
        let mut found = false;
        for previous in &unique {
            if duplicate_eq(previous, value)? {
                representation(previous, value)?;
                found = true;
                break;
            }
        }
        if !found {
            unique.push(value.clone());
        }
    }
    Ok(unique)
}

/// Refuse a visible representative choice instead of inventing independent aggregate outcomes.
fn representation<T: PartialEq>(left: &T, right: &T) -> Result<()> {
    if left != right {
        return Err(error(
            "unsupported.aggregate_representation",
            "SQL-equal values have different observable representations",
        ));
    }
    Ok(())
}

/// Preserve duplicate occurrences unless the call explicitly requests DISTINCT.
fn aggregate(call: &AggregateExpr, rows: &Bag, db: &Database, ctx: &Runtime<'_>) -> Result<Cell> {
    let values = arguments(call, rows, db, ctx)?;
    let values = if call.distinct {
        distinct_inputs(&values)?
    } else {
        values
    };
    calculate(call, &values)
}

/// Apply aggregate-specific empty behavior and widened result types.
fn calculate(call: &AggregateExpr, values: &[Cell]) -> Result<Cell> {
    if call.function == AggregateFunction::Count {
        return Ok(Cell::Int64(values.len() as i64));
    }
    if values.is_empty() {
        return Ok(Cell::Null(call.ty.clone()));
    }
    match call.function {
        AggregateFunction::Min | AggregateFunction::Max => extrema(call, values),
        AggregateFunction::Sum | AggregateFunction::Avg => sum_average(call, values),
        AggregateFunction::Count => unreachable!(),
    }
}

/// Use exact decimal accumulation; integer SUM casts only its final widened value.
fn sum_average(call: &AggregateExpr, values: &[Cell]) -> Result<Cell> {
    let numeric = SqlType::Numeric {
        precision: None,
        scale: None,
    };
    let mut sum = input(&numeric, "0")?;
    for value in values {
        sum = arithmetic("add", &sum, &cast(value, &numeric)?)?;
    }
    if call.function == AggregateFunction::Avg {
        let count = input(&numeric, &values.len().to_string())?;
        sum = arithmetic("divide", &sum, &count)?;
    }
    input(&call.ty, &output(&sum)?)
}

/// Refuse ambiguous tied representations until their cross-call correlations are modeled.
fn extrema(call: &AggregateExpr, values: &[Cell]) -> Result<Cell> {
    let mut best = cast(&values[0], &call.ty)?;
    for value in &values[1..] {
        let value = cast(value, &call.ty)?;
        let order = compare_sql(&value, &best)?;
        let better = if call.function == AggregateFunction::Min {
            order.is_lt()
        } else {
            order.is_gt()
        };
        if better {
            best = value;
        } else if order.is_eq() {
            representation(&best, &value)?;
        }
    }
    Ok(best)
}
