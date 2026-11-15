//! Deterministic lowering of special SQL scalar grammar into admitted typed builtin calls.
use super::*;
use crate::json::array;
use serde_json::json;

impl Expressions<'_> {
    /// EXTRACT retains its exact field and resolves through the immutable scalar signature table.
    pub(super) fn extract(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "field", "arg"])?;
        let field = text(node, "field")?;
        self.bind(&json!({"tag":"call","name":["extract"],"args":[{"tag":"text","value":field},node["arg"]]}),None)
    }

    /// SUBSTRING grammar changes argument order; SIMILAR requires a different matcher.
    pub(super) fn substring(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "form", "args"])?;
        let args = array(node, "args")?;
        let values = match (text(node, "form")?, args.as_slice()) {
            ("from", [value, start]) => vec![value.clone(), start.clone()],
            ("from_for", [value, start, length]) => {
                vec![value.clone(), start.clone(), length.clone()]
            }
            ("for_from", [value, length, start]) => {
                vec![value.clone(), start.clone(), length.clone()]
            }
            ("for_only", [value, length]) => vec![
                value.clone(),
                json!({"tag":"integer","magnitude":"1"}),
                length.clone(),
            ],
            _ => {
                return Err(error(
                    "unsupported.substring_form",
                    "Unsupported SUBSTRING grammar or arity",
                ));
            }
        };
        self.bind(
            &json!({"tag":"call","name":["substring"],"args":values}),
            None,
        )
    }
}
