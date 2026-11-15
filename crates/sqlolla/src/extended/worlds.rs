//! Exhaustive finite input bags; resource limits reject rather than truncate a search.
use crate::pipeline::{Outcome, Request};
use sqlolla_core::{Bag, Cell, Column, Database, Row};

/// Enumerate every closed-catalog input bag using canonical combinations with repetition.
pub(super) fn enumerate(request: &Request) -> Outcome<Vec<Database>> {
    validate_bounds(request)?;
    let mut worlds = vec![Database::new()];
    for (id, table) in &request.catalog.tables {
        let bound = request.bounds[id];
        if bound > 256 {
            return Err(capacity());
        }
        let rows = if bound == 0 {
            Vec::new()
        } else {
            rows(&table.columns, &request.domain)?
        };
        let bags = bags(&rows, bound)?;
        let mut next = Vec::new();
        for world in &worlds {
            for bag in &bags {
                let mut state = world.clone();
                state.insert(id.clone(), bag.clone());
                push(&mut next, state)?;
            }
        }
        worlds = next;
    }
    Ok(worlds)
}

/// A missing bound must not silently restrict an unmentioned table to an empty bag.
fn validate_bounds(request: &Request) -> Outcome<()> {
    if request.bounds.len() != request.catalog.tables.len() {
        return Err((
            "invalid.bounds".into(),
            "explicit outcome analysis requires a bound for every declared table".into(),
        ));
    }
    Ok(())
}

/// Build the complete typed row domain, excluding NULL only for nonnullable columns.
fn rows(columns: &[Column], domain: &[Cell]) -> Outcome<Vec<Row>> {
    let mut rows = vec![Vec::new()];
    for column in columns {
        let mut values: Vec<_> = domain
            .iter()
            .filter(|cell| cell.sql_type() == column.ty && (column.nullable || !cell.is_null()))
            .cloned()
            .collect();
        values.sort();
        values.dedup();
        if values.is_empty() {
            return Err((
                "invalid.domain".into(),
                "a nonzero table bound requires at least one admissible value for each column"
                    .into(),
            ));
        }
        let mut next = Vec::new();
        for row in &rows {
            for value in &values {
                let mut candidate = row.clone();
                candidate.push(value.clone());
                push(&mut next, candidate)?;
            }
        }
        rows = next;
    }
    Ok(rows)
}

/// Include the empty bag and every multiplicity vector with total size up to the bound.
fn bags(rows: &[Row], bound: usize) -> Outcome<Vec<Bag>> {
    let mut result = Vec::new();
    extend_bags(rows, bound, 0, &mut Vec::new(), &mut result)?;
    Ok(result)
}

/// Monotonic row indices remove permutation duplicates without losing any SQL bag.
fn extend_bags(
    rows: &[Row],
    remaining: usize,
    start: usize,
    prefix: &mut Bag,
    output: &mut Vec<Bag>,
) -> Outcome<()> {
    push(output, prefix.clone())?;
    if remaining == 0 {
        return Ok(());
    }
    for index in start..rows.len() {
        prefix.push(rows[index].clone());
        extend_bags(rows, remaining - 1, index, prefix, output)?;
        prefix.pop();
    }
    Ok(())
}

/// Fail before exposing any prefix as a complete search domain.
fn push<T>(values: &mut Vec<T>, value: T) -> Outcome<()> {
    if values.len() == 256 {
        return Err(capacity());
    }
    values.push(value);
    Ok(())
}

/// Keep finite capacity failures distinct from SQL failures and UNSAT results.
fn capacity() -> (String, String) {
    (
        "resource.input_worlds".into(),
        "complete finite input domain exceeds the 256-world capacity".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two values and maximum size two yield six bags, including duplicate occurrences.
    #[test]
    fn all_bag_multiplicities() {
        let rows = vec![vec![Cell::Int32(1)], vec![Cell::Int32(2)]];
        let output = bags(&rows, 2).unwrap();
        assert_eq!(output.len(), 6);
        assert!(output.contains(&vec![rows[0].clone(), rows[0].clone()]));
        assert!(output.contains(&vec![rows[0].clone(), rows[1].clone()]));
    }

    /// A capacity failure cannot produce a falsely complete partial domain.
    #[test]
    fn oversized_domain_refuses() {
        let rows: Vec<_> = (0..16).map(|n| vec![Cell::Int32(n)]).collect();
        assert_eq!(bags(&rows, 3).unwrap_err().0, "resource.input_worlds");
    }
}
