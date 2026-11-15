//! ORDER BY scopes, hidden expressions and typed pagination binding.
use super::*;

impl Binder<'_> {
    /// Bind ordering before pagination and preserve modifiers in nested queries.
    pub(super) fn modified_query(
        &self,
        node: &Value,
        expected: Option<&[SqlType]>,
    ) -> Result<Plan> {
        fields(node, &["tag", "body", "order_by", "limit", "offset"])?;
        let orders = array(node, "order_by")?;
        let body = &node["body"];
        let plan = if text(body, "tag")? == "distinct_on_select" {
            self.distinct_on(body, orders, expected)?
        } else if text(body, "tag")? == "grouped_select" {
            self.grouped_select(body, orders, expected)?
        } else if text(body, "tag")? == "select" && !orders.is_empty() {
            self.ordered_select(body, orders, expected)?
        } else {
            self.order_outputs(self.query(body, expected)?, orders)?
        };
        let plan = self.paginate(plan, node)?;
        let plan = super::set_returning::presentation(plan, body, orders);
        Ok(super::distinct_on::presentation(plan, body, orders))
    }

    /// Attach a positional slice only when the SQL query declares pagination.
    fn paginate(&self, plan: Plan, node: &Value) -> Result<Plan> {
        if node["limit"].is_null() && node["offset"].is_null() {
            return Ok(plan);
        }
        let limit = self.page_size(&node["limit"], "2201W")?;
        let offset = self.page_size(&node["offset"], "2201X")?.unwrap_or(0);
        Ok(Plan {
            schema: plan.schema.clone(),
            node: RelNode::Slice {
                input: Box::new(plan),
                offset,
                limit,
            },
        })
    }

    /// Evaluate constant or explicitly bound integer pagination expressions.
    fn page_size(&self, node: &Value, code: &str) -> Result<Option<usize>> {
        if node.is_null() {
            return Ok(None);
        }
        let expression = self.expressions(&[]).bind(node, None)?;
        let number = match eval_expr(&expression, &Vec::new())? {
            Cell::Null(SqlType::Int32 | SqlType::Int64) => return Ok(None),
            Cell::Int32(value) => i64::from(value),
            Cell::Int64(value) => value,
            _ => {
                return Err(error(
                    "invalid.pagination_type",
                    "LIMIT/OFFSET requires integer",
                ));
            }
        };
        usize::try_from(number)
            .map(Some)
            .map_err(|_| error(&format!("sql.{code}"), "Negative pagination value"))
    }

    /// Bind aliases and ordinals to old projection expressions before ordering.
    fn ordered_select(
        &self,
        node: &Value,
        orders: &[Value],
        expected: Option<&[SqlType]>,
    ) -> Result<Plan> {
        super::set_returning::reject_ordered(node)?;
        let input = self.filtered_source(node)?;
        let (node, input, orders) = self.expanded_windows(node, input, orders)?;
        let (schema, expressions) = self.projections(&node, &input.schema, expected)?;
        let distinct = boolean(&node, "distinct")?;
        let keys = orders
            .iter()
            .map(|order| self.order_key(order, &input.schema, &schema, &expressions, distinct))
            .collect::<Result<Vec<_>>>()?;
        Ok(ordered_projection(
            input,
            schema,
            expressions,
            keys,
            distinct,
        ))
    }

    /// VALUES/set ordering refers only to output positions and labels.
    fn order_outputs(&self, input: Plan, orders: &[Value]) -> Result<Plan> {
        if orders.is_empty() {
            return Ok(input);
        }
        let expressions = input
            .schema
            .iter()
            .enumerate()
            .map(|(index, column)| position(index, column))
            .collect::<Vec<_>>();
        let keys = orders
            .iter()
            .map(|order| self.order_key(order, &input.schema, &input.schema, &expressions, true))
            .collect::<Result<Vec<_>>>()?;
        Ok(order_plan(input, keys))
    }

    /// DISTINCT ordering must resolve to a selected expression rather than a hidden key.
    pub(super) fn order_key(
        &self,
        node: &Value,
        input: &[Column],
        output: &[Column],
        expressions: &[BoundExpr],
        distinct: bool,
    ) -> Result<OrderKey> {
        fields(node, &["expression", "direction", "nulls"])?;
        let selected = selected_position(&node["expression"], output)?;
        let bound = match selected {
            Some(index) => expressions[index].clone(),
            None => self.expressions(input).bind(&node["expression"], None)?,
        };
        let expression = if distinct {
            let index = selected
                .or_else(|| expressions.iter().position(|expr| expr == &bound))
                .ok_or_else(|| error("sql.42P10", "ORDER BY expression is not selected"))?;
            position(index, &output[index])
        } else {
            bound
        };
        let (direction, nulls_first) = ordering_policy(node)?;
        Ok(OrderKey {
            expression,
            direction,
            nulls_first,
        })
    }
}

/// Preserve the required DISTINCT/projection/ordering sequence explicitly.
fn ordered_projection(
    input: Plan,
    schema: Vec<Column>,
    expressions: Vec<BoundExpr>,
    keys: Vec<OrderKey>,
    distinct: bool,
) -> Plan {
    if distinct {
        let projected = project_plan(input, schema.clone(), expressions);
        let unique = Plan {
            schema,
            node: RelNode::Distinct {
                input: Box::new(projected),
            },
        };
        order_plan(unique, keys)
    } else {
        project_plan(order_plan(input, keys), schema, expressions)
    }
}

/// A projection keeps explicit schema and expression position correspondence.
fn project_plan(input: Plan, schema: Vec<Column>, expressions: Vec<BoundExpr>) -> Plan {
    Plan {
        schema,
        node: RelNode::Project {
            input: Box::new(input),
            expressions,
        },
    }
}

/// Ordering leaves column metadata unchanged.
fn order_plan(input: Plan, keys: Vec<OrderKey>) -> Plan {
    Plan {
        schema: input.schema.clone(),
        node: RelNode::Order {
            input: Box::new(input),
            keys,
        },
    }
}

/// PostgreSQL defaults NULLS LAST for ASC and NULLS FIRST for DESC.
pub(super) fn ordering_policy(node: &Value) -> Result<(SortDirection, bool)> {
    let direction = match text(node, "direction")? {
        "asc" => SortDirection::Ascending,
        "desc" => SortDirection::Descending,
        other => return Err(error("invalid.order_direction", other)),
    };
    let nulls = match node["nulls"].as_str() {
        Some("first") => true,
        Some("last") => false,
        None if node["nulls"].is_null() => direction == SortDirection::Descending,
        _ => return Err(error("invalid.nulls_order", "Invalid NULL ordering")),
    };
    Ok((direction, nulls))
}

/// Resolve bare output names before input names, and positive integer ordinals.
fn selected_position(node: &Value, columns: &[Column]) -> Result<Option<usize>> {
    if text(node, "tag")? == "integer" {
        return text(node, "magnitude")?
            .parse::<usize>()
            .ok()
            .filter(|index| *index > 0 && *index <= columns.len())
            .map(|index| Some(index - 1))
            .ok_or_else(|| error("sql.42P10", "ORDER BY position is outside the result"));
    }
    if text(node, "tag")? == "column" && node["qualifier"].is_null() && node["schema"].is_null() {
        let positions = columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name == node["name"])
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        return match positions.as_slice() {
            [index] => Ok(Some(*index)),
            [] => Ok(None),
            _ => Err(error("sql.42702", "ORDER BY output name is ambiguous")),
        };
    }
    Ok(None)
}

/// Construct a positional expression without re-resolving duplicate names.
fn position(index: usize, column: &Column) -> BoundExpr {
    BoundExpr {
        ty: column.ty.clone(),
        nullable: column.nullable,
        node: ExprNode::Column(index),
    }
}
