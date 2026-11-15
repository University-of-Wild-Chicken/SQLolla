//! GROUP BY binding resolves source scope before rewriting to group output positions.
use super::*;
use serde_json::json;

/// Mutable compilation state is local; no database or external state is consulted.
struct Grouping<'a, 'b> {
    binder: &'a Binder<'b>,
    source: &'a [Column],
    columns: Vec<Column>,
    groups: Vec<BoundExpr>,
    calls: Vec<AggregateExpr>,
}

/// Detect current-scope aggregate syntax without crossing a nested query boundary.
pub(super) fn contains_aggregate(node: &Value) -> bool {
    match node.get("tag").and_then(Value::as_str) {
        Some("aggregate" | "count_all" | "filtered_aggregate") => return true,
        Some(
            "query" | "scalar_subquery" | "exists" | "in_query" | "array_query"
            | "quantified_query",
        ) => return false,
        _ => (),
    }
    match node {
        Value::Array(values) => values.iter().any(contains_aggregate),
        Value::Object(values) => values.values().any(contains_aggregate),
        _ => false,
    }
}

impl Binder<'_> {
    /// Bind groups, aggregate arguments, HAVING, projection and ordering in SQL scope order.
    pub(super) fn grouped_select(
        &self,
        node: &Value,
        orders: &[Value],
        expected: Option<&[SqlType]>,
    ) -> Result<Plan> {
        fields(
            node,
            &[
                "tag", "items", "source", "where", "distinct", "group_by", "having",
            ],
        )?;
        if contains_aggregate(&node["where"]) {
            return Err(error("sql.42803", "Aggregate in WHERE"));
        }
        let input = self.filtered_source(node)?;
        let groups = self.group_keys(node, &input.schema)?;
        let binding = Grouping {
            binder: self,
            source: &input.schema,
            columns: input.schema.clone(),
            groups,
            calls: vec![],
        };
        binding.select_plan(node, orders, expected, input.clone())
    }

    /// GROUP BY input names precede output aliases; integer positions address SELECT items.
    fn group_keys(&self, node: &Value, source: &[Column]) -> Result<Vec<BoundExpr>> {
        let mut groups = Vec::new();
        for key in array(node, "group_by")? {
            validate_group_constant(key)?;
            if text(key, "tag")? == "integer" {
                let bound = self.group_ordinal(key, array(node, "items")?, source)?;
                if !groups.contains(&bound) {
                    groups.push(bound);
                }
                continue;
            }
            let key = group_reference(key, array(node, "items")?, source)?;
            if contains_aggregate(&key) {
                return Err(error("sql.42803", "Aggregate in GROUP BY"));
            }
            let expression = self.expressions(source).bind(&key, None)?;
            if !groups.contains(&expression) {
                groups.push(expression);
            }
        }
        Ok(groups)
    }

    /// GROUP BY ordinals count each wildcard-expanded output position separately.
    fn group_ordinal(&self, node: &Value, items: &[Value], source: &[Column]) -> Result<BoundExpr> {
        let mut ordinal = positive_ordinal(node)?;
        for item in items {
            if text(item, "tag")? == "wildcard" {
                let positions = wildcard_positions(item, source)?;
                if ordinal <= positions.len() {
                    let index = positions[ordinal - 1];
                    return Ok(position(index, &source[index]));
                }
                ordinal -= positions.len();
            } else {
                if ordinal == 1 {
                    let value = selected_value(item)?;
                    if contains_aggregate(&value) {
                        return Err(error("sql.42803", "Aggregate in GROUP BY"));
                    }
                    return self.expressions(source).bind(&value, None);
                }
                ordinal -= 1;
            }
        }
        Err(error("sql.42P10", "GROUP BY position outside result"))
    }
}

impl Grouping<'_, '_> {
    /// Compile output expressions before materializing all discovered aggregate slots.
    fn select_plan(
        mut self,
        node: &Value,
        orders: &[Value],
        expected: Option<&[SqlType]>,
        input: Plan,
    ) -> Result<Plan> {
        let (schema, expressions) = self.projections(node, expected)?;
        let having = self.having(&node["having"])?;
        let distinct = boolean(node, "distinct")?;
        let keys = orders
            .iter()
            .map(|order| self.order(order, &schema, &expressions, distinct))
            .collect::<Result<Vec<_>>>()?;
        let filtered = filter_plan(self.plan(input), having);
        Ok(project_order(filtered, schema, expressions, keys, distinct))
    }

    /// Bind a scalar with aggregate placeholders, then enforce group visibility structurally.
    fn expression(&mut self, node: &Value, expected: Option<SqlType>) -> Result<BoundExpr> {
        let value = self.replace_calls(node)?;
        let mut bound = self
            .binder
            .expressions(&self.columns)
            .bind(&value, expected)?;
        self.regroup(&mut bound)?;
        Ok(bound)
    }

    /// Replace complete aggregate nodes; every argument is still bound against source columns.
    fn replace_calls(&mut self, node: &Value) -> Result<Value> {
        if is_aggregate(node) {
            return self.aggregate_reference(node);
        }
        if is_subquery(node) {
            return self.uncorrelated_query(node);
        }
        self.replace_children(node)
    }

    /// Independent nested queries may appear in HAVING without capturing ungrouped source columns.
    fn uncorrelated_query(&self, node: &Value) -> Result<Value> {
        if !matches!(
            node["tag"].as_str(),
            Some("scalar_subquery" | "exists" | "array_query")
        ) {
            return Err(error(
                "unsupported.group_subquery",
                "Grouped quantified correlation needs separate scope rules",
            ));
        }
        let mut expression = self.binder.expressions(&[]).bind(node, None).map_err(|_| {
            error(
                "unsupported.group_subquery",
                "Grouped subquery must bind independently of the current row",
            )
        })?;
        reject_group_correlation(&mut expression)?;
        Ok(node.clone())
    }

    /// Identical aggregate calls share one slot, preserving repeated-reference correlations.
    fn aggregate_reference(&mut self, node: &Value) -> Result<Value> {
        let call = self.aggregate(node)?;
        let index = match self.calls.iter().position(|existing| existing == &call) {
            Some(index) => index,
            None => {
                let index = self.calls.len();
                self.columns.push(synthetic(
                    self.source.len() + index,
                    call.ty.clone(),
                    call.function != AggregateFunction::Count,
                ));
                self.calls.push(call);
                index
            }
        };
        Ok(reference(self.source.len() + index))
    }

    /// Rebuild syntax without modifying the caller's AST or skipping unknown fields.
    fn replace_children(&mut self, node: &Value) -> Result<Value> {
        match node {
            Value::Array(values) => values
                .iter()
                .map(|value| self.replace_calls(value))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            Value::Object(values) => {
                let mut object = serde_json::Map::new();
                for (key, value) in values {
                    object.insert(key.clone(), self.replace_calls(value)?);
                }
                Ok(Value::Object(object))
            }
            _ => Ok(node.clone()),
        }
    }

    /// Whole group expressions are visible, but their ungrouped component columns are not.
    fn regroup(&self, expression: &mut BoundExpr) -> Result<()> {
        if let Some(index) = self.groups.iter().position(|group| group == expression) {
            expression.node = ExprNode::Column(index);
            return Ok(());
        }
        if let ExprNode::Column(index) = &mut expression.node {
            if *index < self.source.len() {
                return Err(error(
                    "sql.42803",
                    "Column must appear in GROUP BY or aggregate",
                ));
            }
            *index = self.groups.len() + *index - self.source.len();
            return Ok(());
        }
        crate::rewrite::scalar_children(expression, &mut |child| self.regroup(child))
    }

    /// FILTER is an independent input predicate and may not contain nested aggregates.
    fn aggregate(&self, node: &Value) -> Result<AggregateExpr> {
        if text(node, "tag")? == "filtered_aggregate" {
            fields(node, &["tag", "aggregate", "predicate"])?;
            if contains_aggregate(&node["predicate"]) {
                return Err(error("sql.42803", "Aggregate in FILTER"));
            }
            let mut call = self.plain_aggregate(&node["aggregate"])?;
            call.filter = Some(
                self.binder
                    .expressions(self.source)
                    .bind(&node["predicate"], Some(SqlType::Bool))?,
            );
            reject_outer_aggregate(call.filter.as_ref().unwrap())?;
            return Ok(call);
        }
        self.plain_aggregate(node)
    }

    /// Resolve only supported built-in overloads, preserving argument and DISTINCT identity.
    fn plain_aggregate(&self, node: &Value) -> Result<AggregateExpr> {
        if text(node, "tag")? == "count_all" {
            fields(node, &["tag"])?;
            return Ok(count_all());
        }
        fields(node, &["tag", "fn", "arg", "distinct"])?;
        if text(node, "tag")? != "aggregate" {
            return Err(error("invalid.aggregate", "Expected aggregate"));
        }
        if contains_aggregate(&node["arg"]) {
            return Err(error("sql.42803", "Nested aggregate"));
        }
        let function = aggregate_function(text(node, "fn")?)?;
        let argument = self.aggregate_argument(function, &node["arg"])?;
        reject_outer_aggregate(&argument)?;
        let ty = aggregate_type(function, &argument.ty)?;
        Ok(AggregateExpr {
            function,
            argument: Some(argument),
            distinct: boolean(node, "distinct")?,
            filter: None,
            ty,
        })
    }

    /// An unresolved unknown argument cannot select a numeric aggregate overload.
    fn aggregate_argument(&self, function: AggregateFunction, node: &Value) -> Result<BoundExpr> {
        let expressions = self.binder.expressions(self.source);
        if matches!(function, AggregateFunction::Sum | AggregateFunction::Avg)
            && expressions.hint(node)?.is_none()
        {
            return Err(error(
                "sql.42725",
                "Aggregate overload is ambiguous for unknown input",
            ));
        }
        expressions.bind(node, None)
    }

    /// Bind projection items in original output order, including qualified wildcard positions.
    fn projections(
        &mut self,
        node: &Value,
        expected: Option<&[SqlType]>,
    ) -> Result<(Vec<Column>, Vec<BoundExpr>)> {
        let mut columns = Vec::new();
        let mut expressions = Vec::new();
        for item in array(node, "items")? {
            if text(item, "tag")? == "wildcard" {
                self.wildcard(item, &mut columns, &mut expressions)?;
            } else {
                let ty = expected.and_then(|types| types.get(columns.len())).cloned();
                let (column, expression) = self.project_item(item, ty)?;
                columns.push(column);
                expressions.push(expression);
            }
        }
        if expected.is_some_and(|types| types.len() != columns.len()) {
            return Err(error("invalid.projection_arity", "Context width mismatch"));
        }
        if columns.is_empty() {
            return Err(error("unsupported.empty_projection", "Empty projection"));
        }
        Ok((columns, expressions))
    }

    /// Validate one scalar projection and retain its original SQL label.
    fn project_item(
        &mut self,
        item: &Value,
        expected: Option<SqlType>,
    ) -> Result<(Column, BoundExpr)> {
        fields(item, &["tag", "value", "alias"])?;
        if text(item, "tag")? != "expression" {
            return Err(error("unsupported.select_item", text(item, "tag")?));
        }
        let expression = self.expression(&item["value"], expected)?;
        let name = crate::json::optional(item, "alias")?
            .unwrap_or_else(|| crate::expression::output_label(&item["value"], &expression));
        Ok((output_column(name, &expression), expression))
    }

    /// Expand wildcards against original input positions before aggregate placeholders exist.
    fn wildcard(
        &self,
        item: &Value,
        columns: &mut Vec<Column>,
        expressions: &mut Vec<BoundExpr>,
    ) -> Result<()> {
        for index in wildcard_positions(item, self.source)? {
            let column = &self.source[index];
            let mut expression = position(index, column);
            self.regroup(&mut expression)?;
            columns.push(output_column(column.name.clone(), &expression));
            expressions.push(expression);
        }
        Ok(())
    }

    /// HAVING sees grouped source expressions and aggregates, never SELECT aliases.
    fn having(&mut self, node: &Value) -> Result<Option<BoundExpr>> {
        if node.is_null() {
            Ok(None)
        } else {
            self.expression(node, Some(SqlType::Bool)).map(Some)
        }
    }

    /// ORDER BY output aliases/ordinals precede grouped source expressions.
    fn order(
        &mut self,
        node: &Value,
        columns: &[Column],
        expressions: &[BoundExpr],
        distinct: bool,
    ) -> Result<OrderKey> {
        fields(node, &["expression", "direction", "nulls"])?;
        let selected = selected_position(&node["expression"], columns, expressions)?;
        let bound = match selected {
            Some(index) => expressions[index].clone(),
            None => self.expression(&node["expression"], None)?,
        };
        let expression = if distinct {
            let index = selected
                .or_else(|| expressions.iter().position(|expr| expr == &bound))
                .ok_or_else(|| error("sql.42P10", "ORDER BY expression is not selected"))?;
            position(index, &columns[index])
        } else {
            bound
        };
        order_key(node, expression)
    }

    /// Materialize the group schema only after SELECT, HAVING and ORDER BY discover all calls.
    fn plan(self, input: Plan) -> Plan {
        let mut schema = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, expression)| synthetic(index, expression.ty.clone(), expression.nullable))
            .collect::<Vec<_>>();
        for call in &self.calls {
            schema.push(synthetic(
                schema.len(),
                call.ty.clone(),
                call.function != AggregateFunction::Count,
            ));
        }
        Plan {
            schema,
            node: RelNode::Aggregate {
                input: Box::new(input),
                groups: self.groups,
                aggregates: self.calls,
            },
        }
    }
}

/// Outer-only aggregate arguments can change the aggregate's SQL query level; refuse that gap.
fn reject_outer_aggregate(expression: &BoundExpr) -> Result<()> {
    if matches!(expression.node, ExprNode::OuterColumn { .. }) {
        return Err(error(
            "unsupported.outer_aggregate_scope",
            "Aggregate outer-column ownership requires separate scope rules",
        ));
    }
    let mut expression = expression.clone();
    crate::rewrite::scalar_children(&mut expression, &mut |child| reject_outer_aggregate(child))
}

/// Refuse nested outer captures until aggregate-level correlation is modeled explicitly.
fn reject_group_correlation(expression: &mut BoundExpr) -> Result<()> {
    if matches!(expression.node, ExprNode::OuterColumn { .. }) {
        return Err(error(
            "unsupported.group_subquery",
            "Grouped subquery contains an outer reference",
        ));
    }
    if let ExprNode::Subquery { query, .. } = &mut expression.node {
        crate::rewrite::relation(query, &mut reject_group_correlation)?;
    }
    crate::rewrite::scalar_children(expression, &mut reject_group_correlation)
}

/// Choose aggregate semantics from the parser's closed built-in aggregate family.
fn aggregate_function(name: &str) -> Result<AggregateFunction> {
    match name {
        "count" => Ok(AggregateFunction::Count),
        "sum" => Ok(AggregateFunction::Sum),
        "avg" => Ok(AggregateFunction::Avg),
        "min" => Ok(AggregateFunction::Min),
        "max" => Ok(AggregateFunction::Max),
        _ => Err(error("unsupported.aggregate", name)),
    }
}

/// PostgreSQL widens exact sums and averages; approximate reductions need a separate model.
fn aggregate_type(function: AggregateFunction, argument: &SqlType) -> Result<SqlType> {
    use AggregateFunction::*;
    if function == Count {
        return Ok(SqlType::Int64);
    }
    if matches!(function, Min | Max) {
        return extremum_type(argument);
    }
    match argument {
        SqlType::Int16 | SqlType::Int32 if function == Sum => Ok(SqlType::Int64),
        SqlType::Int16 | SqlType::Int32 | SqlType::Int64 | SqlType::Numeric { .. } => {
            Ok(SqlType::Numeric {
                precision: None,
                scale: None,
            })
        }
        SqlType::Float64 | SqlType::Interval => Err(error(
            "unsupported.aggregate_family",
            "Aggregate requires a complete rounding/calendar reduction model",
        )),
        _ => Err(error("sql.42883", "Aggregate overload does not exist")),
    }
}

/// MIN/MAX strip storage typmods and admit only PostgreSQL's existing overload families.
fn extremum_type(argument: &SqlType) -> Result<SqlType> {
    match argument {
        SqlType::Varchar { .. } => Ok(SqlType::Text),
        SqlType::Char { .. } => Ok(SqlType::Bpchar),
        SqlType::Numeric { .. } => Ok(SqlType::Numeric {
            precision: None,
            scale: None,
        }),
        SqlType::Bool
        | SqlType::Bytea
        | SqlType::Bit { .. }
        | SqlType::Json
        | SqlType::Jsonb
        | SqlType::Xid => Err(error("sql.42883", "MIN/MAX overload does not exist")),
        SqlType::Array(_) => Err(error(
            "unsupported.aggregate_array",
            "Array extremum overload needs explicit element-family admission",
        )),
        other => Ok(other.clone()),
    }
}

/// Resolve a grouping name to an input column first, then a standalone SELECT alias.
fn group_reference(node: &Value, items: &[Value], source: &[Column]) -> Result<Value> {
    validate_column_reference(node)?;
    if text(node, "tag")? == "column"
        && node["qualifier"].is_null()
        && node["schema"].is_null()
        && !source.iter().any(|column| column.name == node["name"])
    {
        let matches = items
            .iter()
            .filter(|item| item["alias"] == node["name"])
            .collect::<Vec<_>>();
        if let Some(first) = matches.first() {
            let value = selected_value(first)?;
            if matches.iter().any(|item| item["value"] != value) {
                return Err(error("sql.42702", "GROUP BY output alias is ambiguous"));
            }
            return Ok(value);
        }
    }
    Ok(node.clone())
}

/// Validate one selected scalar before using its alias or expanded ordinal as a group key.
fn selected_value(item: &Value) -> Result<Value> {
    if text(item, "tag")? != "expression" {
        return Err(error(
            "unsupported.group_ordinal_wildcard",
            "GROUP BY ordinal over wildcard",
        ));
    }
    fields(item, &["tag", "value", "alias"])?;
    Ok(item["value"].clone())
}

/// Resolve ORDER BY labels and positions while retaining duplicate-name ambiguity.
fn selected_position(
    node: &Value,
    columns: &[Column],
    expressions: &[BoundExpr],
) -> Result<Option<usize>> {
    if text(node, "tag")? == "integer" {
        let index = positive_ordinal(node)?;
        return if index <= columns.len() {
            Ok(Some(index - 1))
        } else {
            Err(error("sql.42P10", "ORDER BY position outside result"))
        };
    }
    validate_column_reference(node)?;
    if text(node, "tag")? == "column" && node["qualifier"].is_null() && node["schema"].is_null() {
        return selected_name(text(node, "name")?, columns, expressions);
    }
    Ok(None)
}

/// Repeated ORDER BY labels are unambiguous only when their bound expressions are identical.
fn selected_name(
    name: &str,
    columns: &[Column],
    expressions: &[BoundExpr],
) -> Result<Option<usize>> {
    let found = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.name == name)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if let Some(first) = found.first() {
        if found
            .iter()
            .any(|index| expressions[*index] != expressions[*first])
        {
            return Err(error("sql.42702", "ORDER BY output name is ambiguous"));
        }
        return Ok(Some(*first));
    }
    Ok(None)
}

/// PostgreSQL rejects bare noninteger GROUP BY constants and negative ordinals before grouping.
pub(super) fn validate_group_constant(node: &Value) -> Result<()> {
    match text(node, "tag")? {
        "text" | "boolean" => fields(node, &["tag", "value"])?,
        "sql_null" => fields(node, &["tag"])?,
        "decimal" => fields(node, &["tag", "lexeme"])?,
        "unary" => fields(node, &["tag", "op", "arg"])?,
        _ => (),
    }
    if matches!(
        text(node, "tag")?,
        "text" | "sql_null" | "boolean" | "decimal"
    ) {
        return Err(error("sql.42601", "Non-integer constant in grouping key"));
    }
    if node["tag"] == "unary" && node["op"] == "negative" && node["arg"]["tag"] == "integer" {
        ordinal_magnitude(&node["arg"])?;
        return Err(error("sql.42P10", "GROUP BY position outside result"));
    }
    Ok(())
}

/// An ordinal is a positive integer syntax node, not a folded arbitrary expression.
pub(super) fn positive_ordinal(node: &Value) -> Result<usize> {
    let value = ordinal_magnitude(node)?;
    if value == 0 {
        return Err(error(
            "sql.42P10",
            "Grouping/ordering position outside result",
        ));
    }
    Ok(value as usize)
}

/// PostgreSQL lexes positive magnitudes beyond signed Int32 as noninteger constants.
fn ordinal_magnitude(node: &Value) -> Result<u32> {
    fields(node, &["tag", "magnitude"])?;
    let value = text(node, "magnitude")?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error("invalid.ast", "Invalid integer magnitude"));
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= i32::MAX as u32)
        .ok_or_else(|| error("sql.42601", "Non-integer constant in grouping/ordering"))
}

/// Alias resolution still validates every field of the original source reference.
fn validate_column_reference(node: &Value) -> Result<()> {
    if text(node, "tag")? == "column" {
        fields(node, &["tag", "name", "qualifier", "schema"])?;
    }
    Ok(())
}

/// Internal identifiers cannot collide with SQL identifiers because PostgreSQL rejects NUL.
fn synthetic(index: usize, ty: SqlType, nullable: bool) -> Column {
    Column {
        name: format!("\0sqlolla.aggregate.{index}"),
        ty,
        nullable,
        qualifier: None,
        schema: None,
    }
}

/// Construct the internal column syntax consumed only by the deterministic scalar binder.
fn reference(index: usize) -> Value {
    json!({"tag":"column","name":format!("\0sqlolla.aggregate.{index}"),"qualifier":null,"schema":null})
}

/// Result columns expose SQL labels and never retain an input qualifier.
fn output_column(name: String, expression: &BoundExpr) -> Column {
    Column {
        name,
        ty: expression.ty.clone(),
        nullable: expression.nullable,
        qualifier: None,
        schema: None,
    }
}

/// Construct a typed positional expression without resolving names again.
fn position(index: usize, column: &Column) -> BoundExpr {
    BoundExpr {
        ty: column.ty.clone(),
        nullable: column.nullable,
        node: ExprNode::Column(index),
    }
}

/// Keep ordering before projection unless DISTINCT requires ordering over selected values.
fn project_order(
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

/// Projection leaves input occurrence multiplicity unchanged.
fn project_plan(input: Plan, schema: Vec<Column>, expressions: Vec<BoundExpr>) -> Plan {
    Plan {
        schema,
        node: RelNode::Project {
            input: Box::new(input),
            expressions,
        },
    }
}

/// HAVING filters aggregate rows only after all aggregate values have been computed.
fn filter_plan(input: Plan, predicate: Option<BoundExpr>) -> Plan {
    match predicate {
        Some(predicate) => Plan {
            schema: input.schema.clone(),
            node: RelNode::Filter {
                input: Box::new(input),
                predicate,
            },
        },
        None => input,
    }
}

/// Aggregate syntax is a closed tag family, never an arbitrary function-name heuristic.
fn is_aggregate(node: &Value) -> bool {
    matches!(
        node.get("tag").and_then(Value::as_str),
        Some("aggregate" | "count_all" | "filtered_aggregate")
    )
}

/// Nested query scopes need a separately bound correlation environment.
fn is_subquery(node: &Value) -> bool {
    matches!(
        node.get("tag").and_then(Value::as_str),
        Some(
            "query"
                | "scalar_subquery"
                | "exists"
                | "in_query"
                | "array_query"
                | "quantified_query"
        )
    )
}

/// COUNT(*) has no scalar argument and always produces non-NULL bigint.
fn count_all() -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Count,
        argument: None,
        distinct: false,
        filter: None,
        ty: SqlType::Int64,
    }
}

/// Wildcard expansion preserves original positions and validates every declared qualifier.
fn wildcard_positions(item: &Value, source: &[Column]) -> Result<Vec<usize>> {
    fields(item, &["tag", "qualifier", "schema"])?;
    let qualifier = crate::json::optional(item, "qualifier")?;
    let schema = crate::json::optional(item, "schema")?;
    crate::expression::qualifier_scope(&qualifier, &schema, source)?;
    let positions = source
        .iter()
        .enumerate()
        .filter(|(_, column)| {
            qualifier
                .as_ref()
                .is_none_or(|q| column.qualifier.as_ref() == Some(q))
                && schema
                    .as_ref()
                    .is_none_or(|s| column.schema.as_ref() == Some(s))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return Err(error("invalid.wildcard", "Wildcard source is not visible"));
    }
    Ok(positions)
}

/// Apply PostgreSQL's default NULLS LAST for ASC and NULLS FIRST for DESC.
fn order_key(node: &Value, expression: BoundExpr) -> Result<OrderKey> {
    let direction = match text(node, "direction")? {
        "asc" => SortDirection::Ascending,
        "desc" => SortDirection::Descending,
        other => return Err(error("invalid.order_direction", other)),
    };
    let nulls_first = match node["nulls"].as_str() {
        Some("first") => true,
        Some("last") => false,
        None if node["nulls"].is_null() => direction == SortDirection::Descending,
        _ => return Err(error("invalid.nulls_order", "Invalid NULL ordering")),
    };
    Ok(OrderKey {
        expression,
        direction,
        nulls_first,
    })
}

/// Empty ORDER BY preserves bag observations rather than claiming ordered output.
fn order_plan(input: Plan, keys: Vec<OrderKey>) -> Plan {
    if keys.is_empty() {
        input
    } else {
        Plan {
            schema: input.schema.clone(),
            node: RelNode::Order {
                input: Box::new(input),
                keys,
            },
        }
    }
}
