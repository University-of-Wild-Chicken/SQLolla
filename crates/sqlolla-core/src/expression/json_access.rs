//! JSONB field and path syntax lowers to one typed structural lookup operation.
use super::*;

impl Expressions<'_> {
    /// Preserve selector overloads and distinguish JSON-returning from text-returning operators.
    pub(super) fn json_access(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "key", "op"])?;
        let mode = text(node, "op")?;
        if self.hint(&node["arg"])?.is_none() {
            return Err(error(
                "unsupported.json_access_unknown",
                "Unknown base needs JSON overload resolution",
            ));
        }
        let base = self.bind(&node["arg"], None)?;
        admit_base(&base.ty)?;
        let key = self.json_selector(&node["key"], mode)?;
        Ok(BoundExpr {
            ty: if mode.ends_with("_text") {
                SqlType::Text
            } else {
                SqlType::Jsonb
            },
            nullable: true,
            node: ExprNode::Call {
                name: "__sqlolla_json_access".into(),
                args: vec![base, key, literal(Cell::Text(mode.into()))],
            },
        })
    }

    /// Unknown field selectors prefer text; typed integer selectors retain array-index meaning.
    fn json_selector(&self, node: &Value, mode: &str) -> Result<BoundExpr> {
        let ty = match mode {
            "path" | "path_text" => SqlType::Array(Box::new(SqlType::Text)),
            "field" | "field_text" => match self.hint(node)? {
                Some(SqlType::Int16 | SqlType::Int32) => SqlType::Int32,
                _ => SqlType::Text,
            },
            _ => {
                return Err(error(
                    "invalid.json_access",
                    "Unknown JSON extraction operator",
                ));
            }
        };
        self.collection_argument(node, ty)
    }
}

/// Plain JSON extraction needs source-span preservation and cannot reuse a canonical JSONB tree.
fn admit_base(ty: &SqlType) -> Result<()> {
    match ty {
        SqlType::Jsonb => Ok(()),
        SqlType::Json => Err(error(
            "unsupported.json_text_extraction",
            "Plain JSON source spans required",
        )),
        _ => Err(error(
            "sql.42883",
            "JSON extraction requires a JSON operand",
        )),
    }
}
