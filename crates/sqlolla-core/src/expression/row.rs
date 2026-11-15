//! Composite comparisons lower explicit constructor components, never nominal records by guesswork.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Resolve comparison overloads independently for every corresponding row component.
    pub(super) fn row_compare(&self, node: &Value) -> Result<BoundExpr> {
        let op = binary_op(text(node, "op")?)?;
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return Err(error("sql.42804", "Boolean row operand"));
        }
        let lhs = row_elements(&node["left"])?;
        let rhs = row_elements(&node["right"])?;
        if lhs.len() != rhs.len() {
            return Err(error("sql.42601", "Unequal row expression lengths"));
        }
        let mut left = Vec::new();
        let mut right = Vec::new();
        for (a, b) in lhs.iter().zip(rhs) {
            let (a, b) = self.row_pair(op, a, b)?;
            left.push(a);
            right.push(b);
        }
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: true,
            node: ExprNode::RowCompare { op, left, right },
        })
    }

    /// PostgreSQL row comparison requires btree membership in addition to scalar equality.
    fn row_pair(&self, op: BinaryOp, a: &Value, b: &Value) -> Result<(BoundExpr, BoundExpr)> {
        let ah = self.hint(a)?;
        let bh = self.hint(b)?;
        super::xid::composite(ah.clone(), bh.clone())?;
        let a_type = ah.or_else(|| bh.clone()).unwrap_or(SqlType::Text);
        let ty = scalar::comparison_type(&a_type, &bh.unwrap_or_else(|| a_type.clone()))?;
        crate::comparison::operator(op, &ty)?;
        if ty == SqlType::Xid {
            return Err(error(
                "sql.0A000",
                "Row equality requires a btree operator family",
            ));
        }
        Ok((self.bind(a, Some(ty.clone()))?, self.bind(b, Some(ty))?))
    }

    /// LIKE operands resolve to text while keeping case policy explicit in the IR.
    pub(super) fn pattern(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "pattern", "insensitive"])?;
        let arg = self.bind(&node["arg"], Some(SqlType::Text))?;
        let pattern = self.bind(&node["pattern"], Some(SqlType::Text))?;
        let insensitive = boolean(node, "insensitive")?;
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: arg.nullable || pattern.nullable,
            node: ExprNode::Pattern {
                arg: Box::new(arg),
                pattern: Box::new(pattern),
                insensitive,
            },
        })
    }
}

/// Reject unmodeled named-record values rather than treating their transport as rows.
fn row_elements(node: &Value) -> Result<&Vec<Value>> {
    fields(node, &["tag", "elements"])?;
    if text(node, "tag")? != "row" {
        return Err(error(
            "unsupported.record_comparison",
            "Expected explicit row constructor",
        ));
    }
    array(node, "elements")
}
