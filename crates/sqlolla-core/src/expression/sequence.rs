//! Sequence calls bind object identity deterministically without allocating during compilation.
use super::*;

impl Expressions<'_> {
    /// Unknown string arguments use strict regclass input before runtime nextval evaluation.
    pub(super) fn sequence_call(&self, node: &Value, name: &str) -> Result<Option<BoundExpr>> {
        if name != "nextval" {
            return Ok(None);
        }
        let args = crate::json::array(node, "args")?;
        let [arg] = args.as_slice() else {
            return Err(error("sql.42883", "nextval requires one regclass argument"));
        };
        let value = self.sequence_argument(arg)?;
        Ok(Some(BoundExpr {
            ty: SqlType::Int64,
            nullable: value.nullable,
            node: ExprNode::Call {
                name: name.into(),
                args: vec![value],
            },
        }))
    }

    /// Literal object names are resolved before execution; NULL remains a strict typed input.
    fn sequence_argument(&self, arg: &Value) -> Result<BoundExpr> {
        let value = if arg["tag"] == "text" {
            literal(crate::catalog_lookup::input(
                &Cell::Text(text(arg, "value")?.into()),
                self.lookup,
            )?)
        } else if arg["tag"] == "sql_null" {
            literal(Cell::Null(SqlType::Regclass))
        } else {
            self.bind(arg, None)?
        };
        if value.ty != SqlType::Regclass {
            return Err(error(
                "unsupported.sequence_argument",
                "Sequence calls require an admitted regclass conversion",
            ));
        }
        Ok(value)
    }
}
