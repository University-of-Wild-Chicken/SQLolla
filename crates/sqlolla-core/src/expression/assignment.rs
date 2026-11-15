//! Assignment coercion is distinct from explicit CAST, especially for character typmods.
use super::*;

impl Expressions<'_> {
    /// Contextually type unknown literals, then retain target typmod enforcement in the IR.
    pub(crate) fn assignment(&self, node: &Value, target: &SqlType) -> Result<BoundExpr> {
        let hint = self.hint(node)?;
        let expected = hint.is_none().then(|| unconstrained(target));
        let arg = self.bind(node, expected)?;
        if !scalar::compatible(&arg.ty, target) {
            return Err(error("sql.42804", "Assignment type mismatch"));
        }
        Ok(BoundExpr {
            ty: target.clone(),
            nullable: arg.nullable,
            node: ExprNode::AssignmentCast { arg: Box::new(arg) },
        })
    }
}

/// Unknown strings acquire a value family before the storage-width check runs.
fn unconstrained(target: &SqlType) -> SqlType {
    match target {
        SqlType::Char { .. } | SqlType::Varchar { .. } => SqlType::Text,
        SqlType::Numeric { .. } => SqlType::Numeric {
            precision: None,
            scale: None,
        },
        _ => target.clone(),
    }
}
