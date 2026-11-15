//! Rectangular slice clipping and immutable array functions retain declared element identity.
use super::{arrays::ArrayValue, rich::Key, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Each endpoint is either omitted or an explicit inclusive integer bound.
type SliceBounds = (Option<i32>, Option<i32>);
/// None distinguishes a reached SQL NULL selector from a vector of omitted endpoints.
type NullableBounds = Option<Vec<SliceBounds>>;

/// Borrow an admitted non-NULL array without normalizing away bounds or dimensions.
pub(crate) fn array(value: &Cell) -> Result<&ArrayValue> {
    match value {
        Cell::Scalar(ScalarValue {
            key: Key::Array(value),
            ..
        }) => Ok(value),
        _ => Err(error("invalid.array_argument", "Expected a non-NULL array")),
    }
}

/// Route array builtins before generic strict-NULL handling because append retains NULLs.
pub(crate) fn function(name: &str, args: &[Cell]) -> Result<Cell> {
    match name {
        "array_append" => append(&args[0], &args[1], false),
        "array_prepend" => append(&args[1], &args[0], true),
        "cardinality" if args[0].is_null() => Ok(Cell::Null(SqlType::Int32)),
        "cardinality" => Ok(Cell::Int32(array(&args[0])?.values.len() as i32)),
        "array_length" => length(args),
        _ => Err(error("invalid.array_function", name)),
    }
}

/// Dimension length is NULL for an absent dimension, including every dimension of empty arrays.
fn length(args: &[Cell]) -> Result<Cell> {
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Int32));
    }
    let Cell::Int32(dimension) = args[1] else {
        return Err(error("invalid.array_dimension", "Dimension must be int32"));
    };
    let length = usize::try_from(i64::from(dimension) - 1)
        .ok()
        .and_then(|index| array(&args[0]).ok()?.dimensions.get(index).copied());
    Ok(length
        .map(|length| Cell::Int32(length as i32))
        .unwrap_or(Cell::Null(SqlType::Int32)))
}

/// Append/prepend only to empty or one-dimensional arrays, preserving a nonempty lower bound.
fn append(base: &Cell, value: &Cell, prepend: bool) -> Result<Cell> {
    let SqlType::Array(element) = base.sql_type() else {
        return Err(error("sql.42883", "Array argument required"));
    };
    let mut array = if base.is_null() {
        ArrayValue::parse(&element, "{}")?
    } else {
        array(base)?.clone()
    };
    validate_append(&array)?;
    let value = super::cast(value, &element)?;
    if prepend {
        array.values.insert(0, value);
    } else {
        array.values.push(value);
    }
    if array.lower.is_empty() {
        array.lower = vec![1];
    }
    array.dimensions = vec![array.values.len()];
    if i64::from(array.lower[0]) + array.values.len() as i64 > i64::from(i32::MAX) {
        return Err(error("sql.54000", "Array upper bound overflow"));
    }
    super::input(&SqlType::Array(element), &array.text()?)
}

/// Check scalar append's dimensional profile and resource capacity before mutation.
fn validate_append(array: &ArrayValue) -> Result<()> {
    if array.dimensions.len() > 1 {
        return Err(error(
            "sql.22000",
            "Argument must be empty or one-dimensional array",
        ));
    }
    if array.values.len() >= 65536 {
        return Err(error("resource.array", "Array element budget exceeded"));
    }
    Ok(())
}

/// Slice arguments encode explicit presence separately from an explicitly NULL bound.
pub(crate) fn slice(args: &[Cell]) -> Result<Cell> {
    let ty = args
        .first()
        .ok_or_else(|| error("invalid.array_slice", "Missing array operand"))?
        .sql_type();
    if args[0].is_null() {
        return Ok(Cell::Null(ty));
    }
    let array = array(&args[0])?;
    let selectors = selectors(&args[1..])?;
    if selectors.is_none() {
        return Ok(Cell::Null(ty));
    }
    let selectors = selectors.unwrap_or_default();
    if selectors.len() > array.dimensions.len() {
        return super::input(&ty, "{}");
    }
    let ranges = ranges(array, &selectors);
    if ranges.is_empty() || ranges.iter().any(Vec::is_empty) {
        return super::input(&ty, "{}");
    }
    slice_result(array, &ranges, &ty)
}

/// Build a fresh rectangular slice with unit lower bounds and exactly the selected occurrences.
fn slice_result(array: &ArrayValue, ranges: &[Vec<usize>], ty: &SqlType) -> Result<Cell> {
    let indices = offsets(ranges, &array.dimensions);
    let sliced = ArrayValue {
        element: array.element.clone(),
        dimensions: ranges.iter().map(Vec::len).collect(),
        lower: vec![1; ranges.len()],
        values: indices
            .into_iter()
            .map(|index| array.values[index].clone())
            .collect(),
    };
    super::input(ty, &sliced.text()?)
}

/// Decode normalized bound-presence flags; omitted endpoints do not become SQL NULLs.
fn selectors(args: &[Cell]) -> Result<NullableBounds> {
    if !args.len().is_multiple_of(4) {
        return Err(error("invalid.array_slice", "Malformed slice arguments"));
    }
    let mut result = Vec::new();
    for group in args.as_chunks::<4>().0 {
        let low = bound(&group[0], &group[1])?;
        let high = bound(&group[2], &group[3])?;
        let (Some(low), Some(high)) = (low, high) else {
            return Ok(None);
        };
        result.push((low, high));
    }
    Ok(Some(result))
}

/// Three states distinguish omission, explicit SQL NULL, and a typed index.
fn bound(present: &Cell, value: &Cell) -> Result<Option<Option<i32>>> {
    match (present, value) {
        (Cell::Bool(false), _) => Ok(Some(None)),
        (Cell::Bool(true), Cell::Null(SqlType::Int32)) => Ok(None),
        (Cell::Bool(true), Cell::Int32(value)) => Ok(Some(Some(*value))),
        _ => Err(error(
            "invalid.array_slice",
            "Invalid slice bound transport",
        )),
    }
}

/// Clip each requested range against original bounds; trailing dimensions are selected in full.
fn ranges(array: &ArrayValue, selectors: &[SliceBounds]) -> Vec<Vec<usize>> {
    array
        .dimensions
        .iter()
        .zip(&array.lower)
        .enumerate()
        .map(|(dimension, (width, lower))| {
            let (start, stop) = selectors.get(dimension).copied().unwrap_or((None, None));
            let lower = i64::from(*lower);
            let start = start.map(i64::from).unwrap_or(lower);
            let stop = stop.map(i64::from).unwrap_or(lower + *width as i64 - 1);
            (0..*width)
                .filter(|offset| start <= lower + *offset as i64 && lower + *offset as i64 <= stop)
                .collect()
        })
        .collect()
}

/// Cartesian coordinate expansion preserves row-major order and never adds elements.
fn offsets(ranges: &[Vec<usize>], widths: &[usize]) -> Vec<usize> {
    ranges
        .iter()
        .zip(widths)
        .fold(vec![0], |offsets, (range, width)| {
            offsets
                .into_iter()
                .flat_map(|offset| range.iter().map(move |index| offset * width + index))
                .collect()
        })
}
