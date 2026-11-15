//! Complete representative choices for DISTINCT ON with SQL duplicate-key equality.
use super::*;

/// Ordered input keeps first representatives; unordered input exposes every representative product.
pub(super) fn evaluate(
    input: &Plan,
    keys: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut output = Vec::new();
    for value in super::relational::query(input, db, context)? {
        let values = super::rel_effects::transform(value, context, |value, context| {
            representatives(value, keys, db, context)
        })?;
        for value in values {
            push(&mut output, value, context)?;
        }
    }
    Ok(output)
}

/// Preserve source order between first groups while grouping NULL and numeric semantic equals.
fn representatives(
    value: RelationResult,
    keys: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let groups = groups(value.rows, keys, db, context)?;
    if value.ordering == ResultOrdering::Ordered {
        let rows = groups
            .into_iter()
            .map(|(_, group)| group[0].clone())
            .collect();
        return Ok(vec![result(rows, ResultOrdering::Ordered)]);
    }
    let mut alternatives = vec![Vec::new()];
    for (_, group) in groups {
        alternatives = append(alternatives, &group, context)?;
    }
    let mut output = Vec::new();
    for rows in alternatives {
        push(&mut output, result(rows, ResultOrdering::Bag), context)?;
    }
    Ok(output)
}

/// Group complete rows by semantic key equality with shared work accounting.
fn groups(
    rows: Bag,
    keys: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<(Row, Bag)>> {
    let mut groups: Vec<(Row, Bag)> = Vec::new();
    for row in rows {
        context.charge()?;
        let key = keys
            .iter()
            .map(|key| scalar(key, &row, db, context))
            .collect::<Result<Row>>()?;
        match group_index(&groups, &key, context)? {
            Some(index) => {
                if !groups[index].1.contains(&row) {
                    groups[index].1.push(row);
                }
            }
            None => groups.push((key, vec![row])),
        }
    }
    Ok(groups)
}

/// Duplicate-key comparison is fallible and consumes shared work fuel for every candidate group.
fn group_index(groups: &[(Row, Bag)], key: &Row, context: &Runtime<'_>) -> Result<Option<usize>> {
    for (index, (prior, _)) in groups.iter().enumerate() {
        context.charge()?;
        if sql_row_equal(prior, key)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// Cartesian representative choices refuse excess rather than dropping a possible winning row.
fn append(prefixes: Vec<Bag>, group: &Bag, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    let mut output = Vec::new();
    for prefix in prefixes {
        for row in group {
            context.charge()?;
            let mut rows = prefix.clone();
            rows.push(row.clone());
            check_rows(rows.len(), context)?;
            push(&mut output, rows, context)?;
        }
    }
    Ok(output)
}
