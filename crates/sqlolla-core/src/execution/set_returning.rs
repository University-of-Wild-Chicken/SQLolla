//! Aligned target-list UNNEST preserves storage order within each input occurrence.
use super::*;

/// Expand every source alternative and retain SQL errors without truncating an output prefix.
pub(super) fn evaluate(
    input: &Plan,
    arrays: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut output = Vec::new();
    for value in super::relational::query(input, db, context)? {
        let values = super::rel_effects::transform(value, context, |value, context| {
            source_orders(value, context)?
                .into_iter()
                .map(|source| match expand(source, arrays, db, context) {
                    Ok(rows) => Ok(result(rows, ResultOrdering::Ordered)),
                    Err(failure) => execution_error(failure),
                })
                .collect()
        })?;
        for value in values {
            push(&mut output, value, context)?;
        }
    }
    Ok(output)
}

/// Unordered input occurrences may reorder; elements of one array must remain contiguous.
fn source_orders(value: RelationResult, context: &Runtime<'_>) -> Result<Vec<Bag>> {
    if value.ordering == ResultOrdering::Ordered {
        Ok(vec![value.rows])
    } else {
        super::ordering::permutations(value.rows, context)
    }
}

/// Evaluate sibling arrays once per input row and append aligned element positions.
fn expand(rows: Bag, arrays: &[BoundExpr], db: &Database, context: &Runtime<'_>) -> Result<Bag> {
    let mut output = Vec::new();
    for row in rows {
        context.charge()?;
        let sets = arrays
            .iter()
            .map(|array| {
                scalar::array_elements(&scalar(array, &row, db, context)?)
                    .map(Option::unwrap_or_default)
            })
            .collect::<Result<Vec<_>>>()?;
        let length = sets.iter().map(Vec::len).max().unwrap_or(0);
        check_rows(output.len().saturating_add(length), context)?;
        for index in 0..length {
            context.charge()?;
            output.push(aligned(&row, &sets, arrays, index)?);
        }
    }
    Ok(output)
}

/// Pad absent sibling ordinals with typed NULL, preserving actual NULL elements unchanged.
fn aligned(source: &Row, sets: &[Vec<Cell>], arrays: &[BoundExpr], index: usize) -> Result<Row> {
    let mut row = source.clone();
    for (set, array) in sets.iter().zip(arrays) {
        let SqlType::Array(element) = &array.ty else {
            return Err(error("invalid.srf_type", "Expected array"));
        };
        row.push(
            set.get(index)
                .cloned()
                .unwrap_or_else(|| Cell::Null(*element.clone())),
        );
    }
    Ok(row)
}
