//! ARRAY expression construction preserves rectangular shape and lower bounds.
use super::arrays::ArrayValue;
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Expose row-major array elements for quantified predicates, retaining typed NULLs.
pub fn array_elements(value: &Cell) -> Result<Option<Vec<Cell>>> {
    match value {
        Cell::Null(SqlType::Array(_)) => Ok(None),
        Cell::Scalar(value) => match &value.key {
            super::rich::Key::Array(array) => Ok(Some(array.values.clone())),
            _ => Err(error("sql.42809", "Array value required")),
        },
        _ => Err(error("sql.42809", "Array value required")),
    }
}

/// Construct scalar or nested ARRAY expressions after binder common-type resolution.
pub fn array_constructor(element: &SqlType, elements: &[Cell]) -> Result<Cell> {
    if elements.len() > 65536 {
        return Err(error("resource.array", "Array element budget exceeded"));
    }
    let array = if let SqlType::Array(base) = element {
        nested(base, elements)?
    } else {
        ArrayValue {
            element: element.clone(),
            dimensions: if elements.is_empty() {
                vec![]
            } else {
                vec![elements.len()]
            },
            lower: if elements.is_empty() { vec![] } else { vec![1] },
            values: elements
                .iter()
                .map(|cell| super::cast(cell, element))
                .collect::<Result<_>>()?,
        }
    };
    super::input(
        &SqlType::Array(Box::new(array.element.clone())),
        &array.text()?,
    )
}

/// NULL and empty child arrays collapse only when every child is empty or NULL.
fn nested(base: &SqlType, elements: &[Cell]) -> Result<ArrayValue> {
    let children = elements
        .iter()
        .map(|cell| child(base, cell))
        .collect::<Result<Vec<_>>>()?;
    let Some(first) = children.first() else {
        return ArrayValue::parse(base, "{}");
    };
    validate_shape(&children, first)?;
    if first.dimensions.is_empty() {
        return ArrayValue::parse(base, "{}");
    }
    let dimensions = std::iter::once(children.len())
        .chain(first.dimensions.iter().copied())
        .collect();
    let lower = std::iter::once(1)
        .chain(first.lower.iter().copied())
        .collect();
    Ok(ArrayValue {
        element: base.clone(),
        dimensions,
        lower,
        values: children
            .into_iter()
            .flat_map(|child| child.values)
            .collect(),
    })
}

/// Resolve one typed child without interpreting NULL as a scalar array element.
fn child(base: &SqlType, cell: &Cell) -> Result<ArrayValue> {
    let text = if cell.is_null() {
        "{}".into()
    } else {
        super::output(&super::cast(cell, &SqlType::Array(Box::new(base.clone())))?)?
    };
    ArrayValue::parse(base, &text)
}

/// Reject ragged arrays and resource overflow before constructing any partial value.
fn validate_shape(children: &[ArrayValue], first: &ArrayValue) -> Result<()> {
    if children
        .iter()
        .any(|child| child.dimensions != first.dimensions || child.lower != first.lower)
    {
        return Err(error(
            "sql.2202E",
            "Multidimensional arrays require matching dimensions",
        ));
    }
    if first.dimensions.len() >= 6
        || children
            .iter()
            .map(|child| child.values.len())
            .sum::<usize>()
            > 65536
    {
        return Err(error(
            "resource.array",
            "Array dimension or element budget exceeded",
        ));
    }
    Ok(())
}
