//! Ranking evaluates complete partition tie alternatives without inventing independent repeated slots.
use super::*;
use std::cmp::Ordering;

/// Lift ranking over every child outcome and preserve explicit SQL/resource failure channels.
pub(super) fn evaluate(
    input: &Plan,
    spec: &WindowSpec,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut outputs = Vec::new();
    for outcome in super::relational::query(input, db, context)? {
        let values = super::rel_effects::transform(outcome, context, |value, context| {
            Ok(ranked(value.rows, spec, db, context)?
                .into_iter()
                .map(|rows| result(rows, ResultOrdering::Bag))
                .collect())
        })?;
        for value in values {
            push(&mut outputs, value, context)?;
        }
    }
    Ok(outputs)
}

/// Partition choices compose independently; capacity refusal occurs before discarding any alternative.
fn ranked(rows: Bag, spec: &WindowSpec, db: &Database, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    let groups = partitions(rows, &spec.partition_by, db, context)?;
    let mut outputs = vec![vec![]];
    for group in groups {
        let choices = partition(group, spec, db, context)?;
        let mut next = Vec::new();
        for prefix in outputs {
            for suffix in &choices {
                context.charge()?;
                let mut rows = prefix.clone();
                rows.extend(suffix.iter().cloned());
                check_rows(rows.len(), context)?;
                rows.sort();
                push(&mut next, rows, context)?;
            }
        }
        outputs = next;
    }
    Ok(outputs)
}

/// SQL duplicate equality, including NULL peers, selects a partition without changing row representation.
fn partitions(
    rows: Bag,
    keys: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    let mut groups: Vec<(Vec<Cell>, Bag)> = Vec::new();
    for row in rows {
        let key = keys
            .iter()
            .map(|key| scalar(key, &row, db, context))
            .collect::<Result<Vec<_>>>()?;
        let mut found = None;
        for (index, (prior, _)) in groups.iter().enumerate() {
            context.charge()?;
            if equal_key(prior, &key)? {
                found = Some(index);
                break;
            }
        }
        if let Some(index) = found {
            groups[index].1.push(row);
        } else {
            groups.push((key, vec![row]));
        }
    }
    Ok(groups.into_iter().map(|(_, rows)| rows).collect())
}

/// Composite partition equality is positional and never Rust transport-string equality.
fn equal_key(left: &[Cell], right: &[Cell]) -> Result<bool> {
    for (left, right) in left.iter().zip(right) {
        if !scalar::duplicate_eq(left, right)? {
            return Ok(false);
        }
    }
    Ok(left.len() == right.len())
}

/// Only ROW_NUMBER observes peer permutations; rank-only windows do not incur factorial work.
fn partition(
    rows: Bag,
    spec: &WindowSpec,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    let keys = rows
        .iter()
        .map(|row| order_values(row, &spec.order_by, db, context))
        .collect::<Result<Vec<_>>>()?;
    let orders = if spec.functions.contains(&WindowFunction::RowNumber) {
        super::ordering::ordered(rows.clone(), &spec.order_by, db, context)?
    } else {
        vec![rows.clone()]
    };
    let mut outputs = Vec::new();
    for order in orders {
        let mut output = Vec::new();
        for (index, row) in order.into_iter().enumerate() {
            let key = order_values(&row, &spec.order_by, db, context)?;
            let (rank, dense) = ranks(&key, &keys, &spec.order_by, context)?;
            output.push(append_slots(row, index + 1, rank, dense, &spec.functions));
        }
        output.sort();
        push(&mut outputs, output, context)?;
    }
    Ok(outputs)
}

/// Order expressions use the shared lazy scalar interpreter and explicit statement context.
fn order_values(
    row: &Row,
    keys: &[OrderKey],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<Cell>> {
    keys.iter()
        .map(|key| scalar(&key.expression, row, db, context))
        .collect()
}

/// Rank counts preceding occurrences; dense rank counts preceding SQL peer classes.
fn ranks(
    key: &[Cell],
    keys: &[Vec<Cell>],
    order: &[OrderKey],
    context: &Runtime<'_>,
) -> Result<(usize, usize)> {
    let mut before = 0;
    let mut distinct: Vec<&Vec<Cell>> = Vec::new();
    for prior in keys {
        context.charge()?;
        if super::ordering::compare(prior, key, order)? != Ordering::Less {
            continue;
        }
        before += 1;
        let mut seen = false;
        for existing in &distinct {
            if super::ordering::compare(existing, prior, order)? == Ordering::Equal {
                seen = true;
                break;
            }
        }
        if !seen {
            distinct.push(prior);
        }
    }
    Ok((before + 1, distinct.len() + 1))
}

/// Every reference to one function reads the same integral slot for this chosen occurrence order.
fn append_slots(
    mut row: Row,
    number: usize,
    rank: usize,
    dense: usize,
    functions: &[WindowFunction],
) -> Row {
    for function in functions {
        let value = match function {
            WindowFunction::RowNumber => number,
            WindowFunction::Rank => rank,
            WindowFunction::DenseRank => dense,
        };
        row.push(Cell::Int64(value as i64));
    }
    row
}
