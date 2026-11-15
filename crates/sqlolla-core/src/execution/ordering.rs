//! Complete ordering and pagination alternatives without selecting an arbitrary tie winner.
use super::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Sort each successful input and enumerate every distinct row order inside equal-key groups.
pub(super) fn order(
    input: &Plan,
    keys: &[OrderKey],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut outputs = Vec::new();
    for outcome in super::relational::query(input, db, context)? {
        let values = super::rel_effects::transform(outcome, context, |value, context| {
            Ok(ordered(value.rows, keys, db, context)?
                .into_iter()
                .map(|rows| result(rows, ResultOrdering::Ordered))
                .collect())
        })?;
        for value in values {
            push(&mut outputs, value, context)?;
        }
    }
    Ok(outputs)
}

/// Evaluate keys once before sorting; comparisons contain no fallible expression execution.
pub(super) fn ordered(
    rows: Bag,
    keys: &[OrderKey],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    let mut keyed = keyed_rows(rows, keys, db, context)?;
    sort_keys(&mut keyed, keys)?;
    let mut groups: Vec<Bag> = Vec::new();
    let mut previous: Option<Vec<Cell>> = None;
    for (row, values) in keyed {
        if match &previous {
            None => true,
            Some(old) => compare(old, &values, keys)? != Ordering::Equal,
        } {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(row);
        previous = Some(values);
    }
    let mut results = vec![Vec::new()];
    for group in groups {
        results = append_group(results, permutations(group, context)?, context)?;
    }
    Ok(results)
}

/// Compute fallible sort keys outside comparisons and preserve the original row occurrence.
fn keyed_rows(
    rows: Bag,
    keys: &[OrderKey],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<(Row, Vec<Cell>)>> {
    rows.into_iter()
        .map(|row| {
            let values = keys
                .iter()
                .map(|key| scalar(&key.expression, &row, db, context))
                .collect::<Result<Vec<_>>>()?;
            Ok((row, values))
        })
        .collect()
}

/// Apply lexicographic SQL NULL placement independently from the direction flag.
pub(super) fn compare(left: &[Cell], right: &[Cell], keys: &[OrderKey]) -> Result<Ordering> {
    for ((a, b), key) in left.iter().zip(right).zip(keys) {
        let order = match (a.is_null(), b.is_null()) {
            (true, true) => Ordering::Equal,
            (true, false) => null_order(key.nulls_first),
            (false, true) => null_order(key.nulls_first).reverse(),
            _ => scalar::compare_sql(a, b)?,
        };
        let order = if !a.is_null() && !b.is_null() && key.direction == SortDirection::Descending {
            order.reverse()
        } else {
            order
        };
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(Ordering::Equal)
}

/// Place one NULL before or after all present values without comparing Rust enum variants.
fn null_order(first: bool) -> Ordering {
    if first {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Fallible insertion sort never converts a type/comparison error into an arbitrary tie.
fn sort_keys(rows: &mut [(Row, Vec<Cell>)], keys: &[OrderKey]) -> Result<()> {
    for index in 1..rows.len() {
        let mut position = index;
        while position > 0 && compare(&rows[position - 1].1, &rows[position].1, keys)?.is_gt() {
            rows.swap(position - 1, position);
            position -= 1;
        }
    }
    Ok(())
}

/// Concatenate all group choices and refuse before losing any permitted order.
fn append_group(prefixes: Vec<Bag>, suffixes: Vec<Bag>, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    let mut results = Vec::new();
    for prefix in prefixes {
        for suffix in &suffixes {
            let mut rows = prefix.clone();
            rows.extend(suffix.iter().cloned());
            push(&mut results, rows, context)?;
        }
    }
    Ok(results)
}

/// Count duplicate rows so identical occurrences do not cause factorial duplicate work.
fn row_counts(rows: Bag) -> Vec<(Row, usize)> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

/// Enumerate every distinct permutation of a finite row multiset.
pub(super) fn permutations(rows: Bag, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    let length = rows.len();
    let mut counts = row_counts(rows);
    let mut results = Vec::new();
    permute(&mut counts, &mut Vec::new(), length, &mut results, context)?;
    Ok(results)
}

/// Depth is bounded by max_rows, and each completed alternative consumes the result budget.
fn permute(
    counts: &mut [(Row, usize)],
    prefix: &mut Bag,
    length: usize,
    results: &mut Vec<Bag>,
    context: &Runtime<'_>,
) -> Result<()> {
    if prefix.len() == length {
        return push(results, prefix.clone(), context);
    }
    for index in 0..counts.len() {
        if counts[index].1 == 0 {
            continue;
        }
        counts[index].1 -= 1;
        prefix.push(counts[index].0.clone());
        permute(counts, prefix, length, results, context)?;
        prefix.pop();
        counts[index].1 += 1;
    }
    Ok(())
}

/// Preserve ordered positions; an unordered page admits every distinct bag of the required size.
pub(super) fn slice(
    input: &Plan,
    offset: usize,
    limit: Option<usize>,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    if limit == Some(0) {
        return Ok(vec![result(vec![], presentation_order(input))]);
    }
    let mut outputs = Vec::new();
    for outcome in super::relational::query(input, db, context)? {
        streaming_error_admission(&outcome, offset, limit)?;
        match outcome {
            Err(failure) => push(&mut outputs, Err(failure), context)?,
            Ok(value) => {
                let ordering = value.ordering;
                let effects = value.effects.clone();
                let pages = pages(value, offset, limit, context)?;
                for rows in pages {
                    let page = super::rel_effects::inherit(result(rows, ordering), &effects);
                    push(&mut outputs, page, context)?;
                }
            }
        }
    }
    Ok(outputs)
}

/// Eager input errors cannot establish complete behavior for a potentially short-circuiting page.
fn streaming_error_admission(
    outcome: &RelationOutcome,
    offset: usize,
    limit: Option<usize>,
) -> Result<()> {
    if outcome.is_err() && (limit.is_some() || offset > 0) {
        return Err(error(
            "unsupported.pagination_error_order",
            "Streaming pagination may avoid an eager input error",
        ));
    }
    Ok(())
}

/// Construct each permitted page while preserving whether source positions are observable.
fn pages(
    value: RelationResult,
    offset: usize,
    limit: Option<usize>,
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    let count = page_count(limit, value.rows.len(), offset);
    if value.ordering == ResultOrdering::Ordered {
        Ok(vec![value
            .rows
            .into_iter()
            .skip(offset)
            .take(count)
            .collect()])
    } else {
        combinations(value.rows, count, context)
    }
}

/// OFFSET beyond the relation leaves an empty page without unsigned underflow.
fn page_count(limit: Option<usize>, length: usize, offset: usize) -> usize {
    limit
        .unwrap_or(usize::MAX)
        .min(length.saturating_sub(offset))
}

/// A zero-row page retains its presentation contract without evaluating runtime expressions.
pub(super) fn presentation_order(plan: &Plan) -> ResultOrdering {
    match &plan.node {
        RelNode::Order { .. } => ResultOrdering::Ordered,
        RelNode::ExpandSets { .. } => ResultOrdering::Ordered,
        RelNode::Project { input, .. }
        | RelNode::Filter { input, .. }
        | RelNode::DistinctOn { input, .. }
        | RelNode::Slice { input, .. } => presentation_order(input),
        _ => ResultOrdering::Bag,
    }
}

/// Enumerate unordered pagination choices directly, avoiding permutations discarded by bag equality.
fn combinations(rows: Bag, length: usize, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    let counts = row_counts(rows);
    let mut results = Vec::new();
    choose(&counts, 0, length, &mut Vec::new(), &mut results, context)?;
    Ok(results)
}

/// Visit each multiset subset once and preserve all original duplicate multiplicities.
fn choose(
    counts: &[(Row, usize)],
    index: usize,
    remaining: usize,
    prefix: &mut Bag,
    results: &mut Vec<Bag>,
    context: &Runtime<'_>,
) -> Result<()> {
    if remaining == 0 {
        return push(results, prefix.clone(), context);
    }
    if index == counts.len() {
        return Ok(());
    }
    let original = prefix.len();
    for amount in 0..=counts[index].1.min(remaining) {
        prefix.truncate(original);
        prefix.extend(std::iter::repeat_n(counts[index].0.clone(), amount));
        choose(
            counts,
            index + 1,
            remaining - amount,
            prefix,
            results,
            context,
        )?;
    }
    prefix.truncate(original);
    Ok(())
}
