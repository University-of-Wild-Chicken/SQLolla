//! Collection syntax lowers to typed pure operations without hidden catalog access.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Known collection signatures supply context only to unknown string/NULL operands.
    pub(super) fn collection_call(&self, node: &Value, name: &str) -> Result<Option<BoundExpr>> {
        if name == "jsonb_set" {
            return self.json_set_call(node).map(Some);
        }
        if !matches!(
            name,
            "jsonb_exists" | "jsonb_exists_any" | "jsonb_exists_all" | "get_bit"
        ) {
            return Ok(None);
        }
        let nodes = array(node, "args")?;
        if nodes.len() != 2 {
            return Err(error("sql.42883", "Collection function arity mismatch"));
        }
        let (first, second) = self.collection_types(name, &nodes[0])?;
        let args = vec![
            self.collection_argument(&nodes[0], first)?,
            self.collection_argument(&nodes[1], second)?,
        ];
        let types = args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>();
        let ty = scalar::return_type(name, &types)?;
        Ok(Some(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Call {
                name: name.into(),
                args,
            },
        }))
    }

    /// jsonb_set's optional create flag defaults to true; only unknowns consume expected types.
    fn json_set_call(&self, node: &Value) -> Result<BoundExpr> {
        let nodes = array(node, "args")?;
        if !(3..=4).contains(&nodes.len()) {
            return Err(error("sql.42883", "jsonb_set arity"));
        }
        let types = [
            SqlType::Jsonb,
            SqlType::Array(Box::new(SqlType::Text)),
            SqlType::Jsonb,
            SqlType::Bool,
        ];
        let args = nodes
            .iter()
            .zip(types)
            .map(|(node, ty)| self.collection_argument(node, ty))
            .collect::<Result<_>>()?;
        Ok(BoundExpr {
            ty: SqlType::Jsonb,
            nullable: true,
            node: ExprNode::Call {
                name: "jsonb_set".into(),
                args,
            },
        })
    }

    /// Resolve collection overloads without converting a typed operand through text.
    fn collection_types(&self, name: &str, first: &Value) -> Result<(SqlType, SqlType)> {
        if name == "get_bit" {
            let ty = self.hint(first)?.ok_or_else(|| {
                error(
                    "unsupported.get_bit_overload",
                    "Unknown first argument needs overload resolution",
                )
            })?;
            return Ok((ty, SqlType::Int32));
        }
        Ok((
            SqlType::Jsonb,
            if name == "jsonb_exists" {
                SqlType::Text
            } else {
                SqlType::Array(Box::new(SqlType::Text))
            },
        ))
    }

    /// Refuse arbitrary explicit casts when resolving an implicit collection signature.
    pub(super) fn collection_argument(&self, node: &Value, expected: SqlType) -> Result<BoundExpr> {
        if let Some(actual) = self.hint(node)? {
            let implicit = actual == expected
                || (expected == SqlType::Int32 && actual == SqlType::Int16)
                || (expected == SqlType::Text
                    && matches!(
                        actual,
                        SqlType::Varchar { .. }
                            | SqlType::Char { .. }
                            | SqlType::Bpchar
                            | SqlType::Name
                    ));
            if !implicit {
                return Err(error("sql.42883", "Collection argument type mismatch"));
            }
        }
        self.bind(node, Some(expected))
    }
    /// Bind JSONB existence operators with their asymmetric operand types.
    pub(super) fn json_existence(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "keys", "op"])?;
        let op = text(node, "op")?;
        let (name, ty) = match op {
            "key" => ("jsonb_exists", SqlType::Text),
            "any" => ("jsonb_exists_any", SqlType::Array(Box::new(SqlType::Text))),
            "all" => ("jsonb_exists_all", SqlType::Array(Box::new(SqlType::Text))),
            _ => return Err(error("invalid.json_existence", op)),
        };
        let args = vec![
            self.collection_argument(&node["arg"], SqlType::Jsonb)?,
            self.collection_argument(&node["keys"], ty)?,
        ];
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: true,
            node: ExprNode::Call {
                name: name.into(),
                args,
            },
        })
    }

    /// Element and slice syntax preserve their distinct SQL result types and shape contracts.
    pub(super) fn subscript(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "base", "indices"])?;
        let base = self.bind(&node["base"], None)?;
        let element = subscript_type(&base.ty)?;
        let indices = array(node, "indices")?;
        if indices.iter().any(|index| index["tag"] == "slice") {
            return self.array_slice(base, indices);
        }
        let mut args = vec![base];
        args.extend(
            indices
                .iter()
                .map(|index| self.index_selector(index))
                .collect::<Result<Vec<_>>>()?,
        );
        Ok(BoundExpr {
            ty: element,
            nullable: true,
            node: ExprNode::Call {
                name: "__sqlolla_array_element".into(),
                args,
            },
        })
    }

    /// Validate one nonslice syntax node before applying assignment-style integer coercion.
    fn index_selector(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "expression"])?;
        if text(node, "tag")? != "index" {
            return Err(error("invalid.array_index", "Expected element index"));
        }
        self.array_index(&node["expression"])
    }

    /// Array indexing uses assignment-style numeric-to-integer coercion, never Boolean casts.
    pub(super) fn array_index(&self, node: &Value) -> Result<BoundExpr> {
        if let Some(ty) = self.hint(node)? {
            if !matches!(
                ty,
                SqlType::Int16
                    | SqlType::Int32
                    | SqlType::Int64
                    | SqlType::Numeric { .. }
                    | SqlType::Float64
            ) {
                return Err(error("sql.42804", "Array subscript must have type integer"));
            }
        }
        self.bind(node, Some(SqlType::Int32))
    }
}

/// JSONB has valid but separately modeled subscripting; other scalar families lack it.
fn subscript_type(ty: &SqlType) -> Result<SqlType> {
    match ty {
        SqlType::Array(element) => Ok(*element.clone()),
        SqlType::Jsonb => Err(error(
            "unsupported.jsonb_subscript",
            "JSONB subscripting needs its separate path contract",
        )),
        _ => Err(error("sql.42804", "Value cannot be subscripted")),
    }
}
