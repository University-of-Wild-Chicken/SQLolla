//! Numeric builtin overloads preserve typed inputs and contextually resolve only unknown literals.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// A single unknown argument selects PostgreSQL's preferred floating numeric overload.
    pub(super) fn absolute_call(&self, node: &Value) -> Result<BoundExpr> {
        let [source] = array(node, "args")?.as_slice() else {
            return Err(error("sql.42883", "Absolute value requires one argument"));
        };
        let expected = self.hint(source)?.is_none().then_some(SqlType::Float64);
        let arg = self.bind(source, expected)?;
        let ty = scalar::return_type("abs", std::slice::from_ref(&arg.ty))?;
        Ok(BoundExpr {
            ty,
            nullable: arg.nullable,
            node: ExprNode::Call {
                name: "abs".into(),
                args: vec![arg],
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind a sign over a typed parameter so no literal shortcut can hide annotation mismatches.
    fn sign(value: Cell, op: &str) -> BoundExpr {
        let params = Parameters::from([(1, value)]);
        let expressions = Expressions {
            lookup: None,
            functions: None,
            columns: &[],
            params: &params,
            extended: true,
            scope: None,
        };
        expressions
            .bind(
                &serde_json::json!({"tag":"unary", "op":op,
            "arg":{"tag":"numbered_parameter", "index":1}}),
                None,
            )
            .unwrap()
    }

    /// Numeric signs discard storage typmods while preserving value display scale and typed NULL.
    #[test]
    fn numeric_sign_annotations_match_values() {
        let stored = SqlType::Numeric {
            precision: Some(4),
            scale: Some(2),
        };
        let family = SqlType::Numeric {
            precision: None,
            scale: None,
        };
        for value in [scalar::input(&stored, "2.10").unwrap(), Cell::Null(stored)] {
            for op in ["positive", "negative"] {
                let expression = sign(value.clone(), op);
                assert_eq!(expression.ty, family);
                let result = eval_expr(&expression, &vec![]).unwrap();
                assert_eq!(result.sql_type(), family);
                assert_eq!(result.is_null(), value.is_null());
                if !result.is_null() {
                    assert_eq!(
                        scalar::output(&result).unwrap(),
                        if op == "negative" { "-2.10" } else { "2.10" }
                    );
                }
            }
        }
    }
}
