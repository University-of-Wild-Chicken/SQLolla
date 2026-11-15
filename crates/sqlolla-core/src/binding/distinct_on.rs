//! DISTINCT ON retains key expressions and validates PostgreSQL's leading ORDER BY requirement.
use super::*;

impl Binder<'_> {
    /// Select representatives before evaluating output expressions that are not sorting keys.
    pub(super) fn distinct_on(
        &self,
        node: &Value,
        orders: &[Value],
        expected: Option<&[SqlType]>,
    ) -> Result<Plan> {
        fields(node, &["tag", "items", "source", "where", "distinct_on"])?;
        let input = self.filtered_source(node)?;
        let (schema, expressions) = self.projections(node, &input.schema, expected)?;
        let keys = self.distinct_keys(node, &input.schema, &schema, &expressions)?;
        let orders = orders
            .iter()
            .map(|order| self.order_key(order, &input.schema, &schema, &expressions, false))
            .collect::<Result<Vec<_>>>()?;
        validate_prefix(&keys, &orders)?;
        let input = ordered(input, complete_order(orders, &keys));
        Ok(Plan {
            schema,
            node: RelNode::Project {
                input: Box::new(unique(input, keys)),
                expressions,
            },
        })
    }

    /// Resolve key aliases and ordinals while retaining PostgreSQL's literal syntax errors.
    fn distinct_keys(
        &self,
        node: &Value,
        source: &[Column],
        schema: &[Column],
        expressions: &[BoundExpr],
    ) -> Result<Vec<BoundExpr>> {
        array(node, "distinct_on")?
            .iter()
            .map(|key| {
                super::grouping::validate_group_constant(key)?;
                if key["tag"] == "integer" {
                    super::grouping::positive_ordinal(key)?;
                }
                let order = serde_json::json!({"expression":key,"direction":"asc","nulls":null});
                self.order_key(&order, source, schema, expressions, false)
                    .map(|key| key.expression)
            })
            .collect()
    }
}

/// PostgreSQL16 appends missing DISTINCT ON keys with their default ascending sort semantics.
fn complete_order(mut orders: Vec<OrderKey>, keys: &[BoundExpr]) -> Vec<OrderKey> {
    for key in keys {
        if !orders.iter().any(|order| &order.expression == key) {
            orders.push(OrderKey {
                expression: key.clone(),
                direction: SortDirection::Ascending,
                nulls_first: false,
            });
        }
    }
    orders
}

/// The implicit selection order affects pagination but does not promise SQL presentation order.
pub(super) fn presentation(plan: Plan, body: &Value, orders: &[Value]) -> Plan {
    if body["tag"] == "distinct_on_select" && orders.is_empty() {
        Plan {
            schema: plan.schema.clone(),
            node: RelNode::Unordered {
                input: Box::new(plan),
            },
        }
    } else {
        plan
    }
}

/// Retain the complete source row until selection has chosen one representative per key.
fn unique(input: Plan, keys: Vec<BoundExpr>) -> Plan {
    Plan {
        schema: input.schema.clone(),
        node: RelNode::DistinctOn {
            input: Box::new(input),
            keys,
        },
    }
}

/// Each leading ORDER BY expression must be a DISTINCT ON key until all keys have occurred.
fn validate_prefix(keys: &[BoundExpr], orders: &[OrderKey]) -> Result<()> {
    if keys.is_empty() {
        return Err(error("invalid.distinct_on", "DISTINCT ON requires keys"));
    }
    let mut matched = Vec::new();
    for order in orders {
        if keys.iter().any(|key| key == &order.expression) {
            matched.push(&order.expression);
        } else if !keys.iter().all(|key| matched.contains(&key)) {
            return Err(error(
                "sql.42P10",
                "DISTINCT ON expressions must match initial ORDER BY expressions",
            ));
        }
    }
    Ok(())
}

/// Ordering remains an explicit operator, including every equal-key alternative.
fn ordered(input: Plan, keys: Vec<OrderKey>) -> Plan {
    if keys.is_empty() {
        return input;
    }
    Plan {
        schema: input.schema.clone(),
        node: RelNode::Order {
            input: Box::new(input),
            keys,
        },
    }
}
