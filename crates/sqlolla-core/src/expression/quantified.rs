//! Array quantifiers resolve one comparison family and preserve NULL-array identity.
use super::*;

impl Expressions<'_> {
    /// Bind ANY/SOME/ALL arrays without collapsing SQL NULL into an empty collection.
    pub(super) fn quantified(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "op", "quantifier", "left", "right"])?;
        let op = binary_op(text(node, "op")?)?;
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return Err(error("sql.42883", "Invalid quantified operator"));
        }
        let all = quantifier(text(node, "quantifier")?)?;
        let ty = self.quantified_type(node)?;
        crate::comparison::operator(op, &ty)?;
        let left = self.bind(&node["left"], Some(ty.clone()))?;
        let right = self.bind(&node["right"], Some(SqlType::Array(Box::new(ty))))?;
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: true,
            node: ExprNode::Quantified {
                op,
                all,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// Comparison resolution uses the array element family, never the array transport type.
    fn quantified_type(&self, node: &Value) -> Result<SqlType> {
        let left_type = self.hint(&node["left"])?;
        let right_type = self.hint(&node["right"])?;
        let element = match right_type {
            Some(SqlType::Array(element)) => *element,
            None => left_type.clone().unwrap_or(SqlType::Text),
            _ => {
                return Err(error(
                    "sql.42809",
                    "Quantified comparison requires an array",
                ));
            }
        };
        super::xid::composite(left_type.clone(), Some(element.clone()))?;
        scalar::comparison_type(&left_type.unwrap_or_else(|| element.clone()), &element)
    }
}

/// SOME is an alternate spelling of ANY; every other label must be rejected.
fn quantifier(value: &str) -> Result<bool> {
    match value {
        "all" => Ok(true),
        "any" | "some" => Ok(false),
        _ => Err(error("invalid.quantifier", "Unknown quantifier")),
    }
}
