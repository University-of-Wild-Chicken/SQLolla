//! Known text/collection signatures contextually type unknown literals without arbitrary casts.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Resolve exact string-array and JSONB inspection overloads before ordinary call binding.
    pub(super) fn string_collection_call(
        &self,
        node: &Value,
        name: &str,
    ) -> Result<Option<BoundExpr>> {
        if !matches!(name, "string_to_array" | "array_to_string" | "jsonb_typeof") {
            return Ok(None);
        }
        let nodes = array(node, "args")?;
        let targets = self.string_collection_types(name, nodes)?;
        let args = nodes
            .iter()
            .zip(targets)
            .map(|(node, ty)| self.collection_argument(node, ty))
            .collect::<Result<Vec<_>>>()?;
        let ty = scalar::return_type(
            name,
            &args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>(),
        )?;
        Ok(Some(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Call {
                name: name.into(),
                args,
            },
        }))
    }

    /// Polymorphic array input must already resolve; the delimiter and marker are text.
    fn string_collection_types(&self, name: &str, nodes: &[Value]) -> Result<Vec<SqlType>> {
        if name == "jsonb_typeof" {
            return if nodes.len() == 1 {
                Ok(vec![SqlType::Jsonb])
            } else {
                Err(error("sql.42883", "jsonb_typeof arity"))
            };
        }
        if !(2..=3).contains(&nodes.len()) {
            return Err(error("sql.42883", "String/array function arity"));
        }
        let mut types = vec![SqlType::Text; nodes.len()];
        if name == "array_to_string" {
            let Some(ty @ SqlType::Array(_)) = self.hint(&nodes[0])? else {
                return Err(error(
                    "sql.42804",
                    "Cannot resolve polymorphic array argument",
                ));
            };
            types[0] = ty;
        }
        Ok(types)
    }
}
