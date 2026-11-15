//! Context-aware effect signatures preserve unknown literals and reject explicit wrong types.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Bind only modeled effect families; evaluation remains branch-local and context-driven.
    pub(super) fn session_effect_call(
        &self,
        node: &Value,
        name: &str,
    ) -> Result<Option<BoundExpr>> {
        if name == "pg_terminate_backend" {
            return self.backend_call(node).map(Some);
        }
        if !crate::is_session_effect(name) {
            return Ok(None);
        }
        let nodes = array(node, "args")?;
        let targets = signature(name, nodes.len())?;
        let args = nodes
            .iter()
            .zip(targets)
            .map(|(node, target)| self.effect_argument(node, target))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(BoundExpr {
            ty: SqlType::Void,
            nullable: name != "pg_notify" && args.iter().any(|arg| arg.nullable),
            node: ExprNode::Call {
                name: name.into(),
                args,
            },
        }))
    }

    /// PostgreSQL supplies a zero bigint timeout when the optional second argument is absent.
    fn backend_call(&self, node: &Value) -> Result<BoundExpr> {
        let nodes = array(node, "args")?;
        if !(1..=2).contains(&nodes.len()) {
            return Err(error("sql.42883", "Backend function arity mismatch"));
        }
        let mut args = nodes
            .iter()
            .zip([SqlType::Int32, SqlType::Int64])
            .map(|(node, target)| self.effect_argument(node, target))
            .collect::<Result<Vec<_>>>()?;
        if args.len() == 1 {
            args.push(literal(Cell::Int64(0)));
        }
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: args.iter().any(|arg| arg.nullable),
            node: ExprNode::Call {
                name: "pg_terminate_backend".into(),
                args,
            },
        })
    }

    /// SQL NULL syntax consumes overload context; explicitly typed NULL does not evade type checking.
    fn effect_argument(&self, node: &Value, target: SqlType) -> Result<BoundExpr> {
        if let Some(source) = self.hint(node)? {
            let allowed = source == target
                || matches!(
                    (&source, &target),
                    (SqlType::Int16, SqlType::Int32 | SqlType::Int64)
                        | (SqlType::Int32, SqlType::Int64)
                )
                || (target == SqlType::Text
                    && matches!(
                        source,
                        SqlType::Name
                            | SqlType::Varchar { .. }
                            | SqlType::Char { .. }
                            | SqlType::Bpchar
                    ));
            if !allowed {
                return Err(error("sql.42883", "Effect function signature mismatch"));
            }
        }
        self.bind(node, Some(target))
    }
}

/// Advisory overload arity selects its distinct bigint or int-pair key namespace.
fn signature(name: &str, arity: usize) -> Result<Vec<SqlType>> {
    match (name, arity) {
        ("pg_notify", 2) => Ok(vec![SqlType::Text, SqlType::Text]),
        ("pg_advisory_xact_lock" | "pg_advisory_xact_lock_shared", 1) => Ok(vec![SqlType::Int64]),
        ("pg_advisory_xact_lock" | "pg_advisory_xact_lock_shared", 2) => {
            Ok(vec![SqlType::Int32, SqlType::Int32])
        }
        _ => Err(error("sql.42883", "Effect function arity mismatch")),
    }
}
