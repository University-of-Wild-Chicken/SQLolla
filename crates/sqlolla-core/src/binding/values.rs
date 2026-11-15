//! VALUES column type resolution and finite literal row binding.
use super::*;

impl Binder<'_> {
    /// VALUES resolves each column locally before an enclosing set operation.
    pub(super) fn value_types(&self, node: &Value) -> Result<Vec<SqlType>> {
        fields(node, &["tag", "rows"])?;
        let rows = array(node, "rows")?;
        let first = rows
            .first()
            .and_then(Value::as_array)
            .ok_or_else(|| error("invalid.values", "VALUES needs rows"))?;
        if first.is_empty() {
            return Err(error("invalid.values", "VALUES needs columns"));
        }
        let mut types = vec![None; first.len()];
        for row in rows {
            let cells = row
                .as_array()
                .ok_or_else(|| error("invalid.values", "Expected VALUES row"))?;
            if cells.len() != types.len() {
                return Err(error("invalid.values", "VALUES widths differ"));
            }
            for (index, value) in cells.iter().enumerate() {
                if let Some(ty) = self.expressions(&[]).hint(value)? {
                    types[index] = Some(common(types[index].clone(), Some(ty))?);
                }
            }
        }
        Ok(types
            .into_iter()
            .map(|ty| ty.unwrap_or(SqlType::Text))
            .collect())
    }

    /// Bind complete constant rows and retain duplicate occurrences.
    pub(super) fn values(&self, node: &Value, expected: Option<&[SqlType]>) -> Result<Plan> {
        let types = self.value_types(node)?;
        if expected.is_some_and(|other| other != types.as_slice()) {
            return Err(error("invalid.type", "VALUES type mismatch"));
        }
        let schema = value_schema(&types);
        if self.extended {
            let rows = array(node, "rows")?
                .iter()
                .map(|row| self.bound_value_row(row, &types))
                .collect::<Result<_>>()?;
            return Ok(Plan {
                schema,
                node: RelNode::EvaluateValues { rows },
            });
        }
        let rows = array(node, "rows")?
            .iter()
            .map(|row| self.value_row(row, &types))
            .collect::<Result<_>>()?;
        Ok(Plan {
            schema,
            node: RelNode::Values { rows },
        })
    }

    /// Evaluate only bound constant expressions; column references have no scope.
    pub(super) fn value_row(&self, row: &Value, types: &[SqlType]) -> Result<Row> {
        self.bound_value_row(row, types)?
            .iter()
            .map(|bound| eval_expr(bound, &vec![]))
            .collect()
    }

    /// Preserve scalar expression trees until planner simplification and context substitution.
    pub(super) fn bound_value_row(&self, row: &Value, types: &[SqlType]) -> Result<Vec<BoundExpr>> {
        let cells = row
            .as_array()
            .ok_or_else(|| error("invalid.values", "Expected VALUES row"))?;
        cells
            .iter()
            .zip(types)
            .map(|(value, ty)| self.expressions(&[]).bind(value, Some(ty.clone())))
            .collect()
    }
}

/// VALUES assigns positional labels independently of the literal spelling or representation.
pub(super) fn value_schema(types: &[SqlType]) -> Vec<Column> {
    types
        .iter()
        .enumerate()
        .map(|(index, ty)| Column {
            name: format!("column{}", index + 1),
            ty: ty.clone(),
            nullable: true,
            qualifier: None,
            schema: None,
        })
        .collect()
}
