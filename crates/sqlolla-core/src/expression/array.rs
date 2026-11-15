//! SQL ARRAY constructors retain element typing and dimensionality as explicit IR.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Resolve constructor element families before binding unknown literals or empty arrays.
    pub(super) fn array(&self, node: &Value, expected: Option<SqlType>) -> Result<BoundExpr> {
        fields(node, &["tag", "elements"])?;
        let nodes = array(node, "elements")?;
        let element = self.array_element(nodes, expected)?;
        let values = nodes
            .iter()
            .map(|value| self.bind(value, Some(element.clone())))
            .collect::<Result<Vec<_>>>()?;
        let ty = match &element {
            SqlType::Array(_) => element.clone(),
            _ => SqlType::Array(Box::new(element.clone())),
        };
        Ok(BoundExpr {
            ty,
            nullable: false,
            node: ExprNode::Array { element, values },
        })
    }

    /// Known operands determine a common family; only unknown operands consume context.
    fn array_element(&self, nodes: &[Value], expected: Option<SqlType>) -> Result<SqlType> {
        let expected = match expected {
            Some(SqlType::Array(value)) => Some(*value),
            Some(_) => return Err(error("sql.42804", "ARRAY context is not an array")),
            None => None,
        };
        let mut ty = None;
        for node in nodes {
            if let Some(next) = self.hint(node)? {
                ty = Some(match ty {
                    None => next,
                    Some(old) => scalar::common_type(&old, &next)?,
                });
            }
        }
        let fallback = if nodes.is_empty() {
            None
        } else {
            Some(SqlType::Text)
        };
        ty.or(expected)
            .or(fallback)
            .ok_or_else(|| error("sql.42P18", "Cannot determine empty ARRAY type"))
    }
}
