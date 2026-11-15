//! INSERT DEFAULT is a typed per-occurrence request, not an ordinary NULL expression.
use super::*;

/// Extract explicit literal-source defaults while retaining every source occurrence's position.
pub(super) fn source(node: &Value, columns: &[usize]) -> Result<(Value, Vec<Vec<usize>>)> {
    if node["tag"] == "default_values" {
        fields(node, &["tag"])?;
        let row = vec![serde_json::json!({"tag":"sql_null"}); columns.len()];
        return Ok((
            serde_json::json!({"tag":"values", "rows":[row]}),
            vec![columns.to_vec()],
        ));
    }
    if node["tag"] != "values" {
        return Ok((node.clone(), vec![]));
    }
    fields(node, &["tag", "rows"])?;
    let mut input = node.clone();
    let rows = input["rows"]
        .as_array_mut()
        .ok_or_else(|| error("invalid.ast", "Missing VALUES rows"))?;
    let masks = rows
        .iter_mut()
        .map(|row| positions(row, columns))
        .collect::<Result<Vec<_>>>()?;
    Ok((input, masks))
}

/// Typed NULL placeholders are replaced at execution; explicit SQL NULL has no default marker.
fn positions(node: &mut Value, columns: &[usize]) -> Result<Vec<usize>> {
    let row = node
        .as_array_mut()
        .ok_or_else(|| error("invalid.ast", "Expected VALUES row"))?;
    if row.len() != columns.len() {
        return Err(error("sql.42601", "INSERT VALUES width mismatch"));
    }
    let mut defaults = Vec::new();
    for (value, column) in row.iter_mut().zip(columns) {
        if value["tag"] == "default_value" {
            fields(value, &["tag"])?;
            defaults.push(*column);
            *value = serde_json::json!({"tag":"sql_null"});
        }
    }
    Ok(defaults)
}
