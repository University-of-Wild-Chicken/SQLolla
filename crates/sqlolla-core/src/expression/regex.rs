//! Regex operators retain strict text signatures and explicit match flags.
use super::*;

impl Expressions<'_> {
    /// Resolve PostgreSQL regex operands without accepting arbitrary implicit text casts.
    pub(super) fn regex(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "pattern", "insensitive", "negated"])?;
        let left = self.collection_argument(&node["arg"], SqlType::Text)?;
        let right = self.collection_argument(&node["pattern"], SqlType::Text)?;
        let nullable = left.nullable || right.nullable;
        let args = vec![
            left,
            right,
            literal(Cell::Bool(boolean(node, "insensitive")?)),
            literal(Cell::Bool(boolean(node, "negated")?)),
        ];
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable,
            node: ExprNode::Call {
                name: "__sqlolla_regex".into(),
                args,
            },
        })
    }
}
