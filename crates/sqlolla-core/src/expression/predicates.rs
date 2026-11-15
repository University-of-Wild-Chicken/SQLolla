//! Pure predicate lowering reuses checked comparison, NULL and three-valued Boolean semantics.
use super::*;
use crate::json::array;
use serde_json::json;

impl Expressions<'_> {
    /// IN is the three-valued disjunction of equalities, including NULL list members.
    pub(super) fn in_list(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "values"])?;
        let values = array(node, "values")?;
        if values.is_empty() {
            return Err(error("sql.42601", "Empty IN list"));
        }
        let mut comparisons = values
            .iter()
            .map(|value| binary("eq", node["arg"].clone(), value.clone()));
        let first = comparisons.next().unwrap();
        let value = comparisons.fold(first, |left, right| binary("or", left, right));
        self.bind(&value, Some(SqlType::Bool))
    }

    /// BETWEEN retains UNKNOWN through comparisons; NOT applies SQL three-valued negation.
    pub(super) fn between(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "lower", "upper", "negated"])?;
        let low = binary("ge", node["arg"].clone(), node["lower"].clone());
        let high = binary("le", node["arg"].clone(), node["upper"].clone());
        let value = binary("and", low, high);
        let value = if boolean(node, "negated")? {
            json!({"tag":"unary","op":"not","arg":value})
        } else {
            value
        };
        self.bind(&value, Some(SqlType::Bool))
    }

    /// IS DISTINCT FROM is total even when either comparison operand is NULL.
    pub(super) fn distinct_from(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "left", "right", "negated"])?;
        let left = &node["left"];
        let right = &node["right"];
        let a = json!({"tag":"is_null","arg":left,"negated":false});
        let b = json!({"tag":"is_null","arg":right,"negated":false});
        let value = json!({"tag":"case","operand":null,"branches":[
            {"when":a,"then":{"tag":"is_null","arg":right,"negated":true}},
            {"when":b,"then":{"tag":"boolean","value":true}}],
            "else":binary("ne",left.clone(),right.clone())});
        let value = if boolean(node, "negated")? {
            json!({"tag":"unary","op":"not","arg":value})
        } else {
            value
        };
        let mut bound = self.bind(&value, Some(SqlType::Bool))?;
        bound.nullable = false;
        Ok(bound)
    }
}

/// Construct only structural parser nodes, never SQL strings or an independent SQL parser.
fn binary(op: &str, left: Value, right: Value) -> Value {
    json!({"tag":"binary","op":op,"left":left,"right":right})
}
