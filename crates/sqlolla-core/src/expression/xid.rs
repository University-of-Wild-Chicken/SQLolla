//! Transaction identifiers have equality overloads but no implicit integer cast or ordering.
use super::*;

/// Composite adapters must not turn an unimplemented operator overload into a fabricated SQL error.
pub(super) fn composite(left: Option<SqlType>, right: Option<SqlType>) -> Result<()> {
    if let (Some(left), Some(right)) = (left, right) {
        if left != right && (left == SqlType::Xid || right == SqlType::Xid) {
            return Err(error(
                "unsupported.xid_composite_overload",
                "Mixed xid comparison requires a typed operator adapter",
            ));
        }
    }
    Ok(())
}

impl Expressions<'_> {
    /// Resolve xid/xid and xid/int4 without disguising raw-bit comparison as a cast.
    pub(super) fn xid_comparison(&self, node: &Value, op: BinaryOp) -> Result<Option<BoundExpr>> {
        let left_type = self.hint(&node["left"])?;
        let right_type = self.hint(&node["right"])?;
        if left_type == Some(SqlType::Xid) && right_type == Some(SqlType::Int16) {
            return Err(error(
                "unsupported.xid_smallint_overload",
                "Implicit int2-to-int4 operator widening is not yet admitted",
            ));
        }
        if left_type != Some(SqlType::Xid) && right_type != Some(SqlType::Xid) {
            return Ok(None);
        }
        let left = self.bind(&node["left"], left_type.or(Some(SqlType::Xid)))?;
        let right = self.bind(&node["right"], right_type.or(Some(SqlType::Xid)))?;
        if !matches!(op, BinaryOp::Eq | BinaryOp::Ne)
            || left.ty != SqlType::Xid
            || !matches!(right.ty, SqlType::Xid | SqlType::Int32)
        {
            return Err(error("sql.42883", "No matching xid operator"));
        }
        Ok(Some(BoundExpr {
            ty: SqlType::Bool,
            nullable: left.nullable || right.nullable,
            node: ExprNode::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        }))
    }
}
