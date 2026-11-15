//! Statement CTEs use reserved virtual scans and retain same-snapshot execution boundaries.
use super::*;

impl Binder<'_> {
    /// Bind nested statement syntax under the current lexical CTE environment.
    pub(super) fn statement_node(&self, node: &Value) -> Result<StatementPlan> {
        self.charge()?;
        match text(node, "tag")? {
            "with_statement" => self.with_statement(node).map(StatementPlan::With),
            "query" if locking_ctes(node)? => self.locking_with(node),
            "query" if !array(node, "locking")?.is_empty() => {
                self.locked_query(node).map(StatementPlan::LockedQuery)
            }
            "query" => self
                .query(&normalize::query(node)?, None)
                .map(StatementPlan::Query),
            _ => self.mutation(node),
        }
    }

    /// Materialized lock CTEs become explicit statement bindings rather than duplicated pure plans.
    fn locking_with(&self, node: &Value) -> Result<StatementPlan> {
        let mut body = node.clone();
        body["ctes"] = serde_json::json!([]);
        let mut ctes = Vec::new();
        for cte in array(node, "ctes")? {
            fields(cte, &["name", "columns", "query"])?;
            ctes.push(
                serde_json::json!({"name":cte["name"],"columns":cte["columns"],
                "materialized":null,"statement":cte["query"]}),
            );
        }
        let syntax = serde_json::json!({"tag":"with_statement","recursive":node["recursive"],
            "ctes":ctes,"body":body});
        self.with_statement(&syntax).map(StatementPlan::With)
    }

    /// Recursive or materialization-policy variants remain explicit admission failures.
    fn with_statement(&self, node: &Value) -> Result<WithPlan> {
        fields(node, &["tag", "recursive", "ctes", "body"])?;
        if boolean(node, "recursive")? {
            return Err(error("unsupported.recursive", "Recursive statement CTE"));
        }
        let mut scope = self.clone();
        let mut bindings = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for item in array(node, "ctes")? {
            let name = text(item, "name")?;
            if !names.insert(name) {
                return Err(error("sql.42712", "Duplicate CTE name"));
            }
            bindings.push(scope.statement_cte(item)?);
        }
        let body = Box::new(scope.statement_node(&node["body"])?);
        let mut plan = WithPlan { bindings, body };
        crate::cte::retain_demanded(&mut plan)?;
        Ok(plan)
    }

    /// A CTE alias exposes only its result schema, while base tables remain unresolved scans.
    fn statement_cte(&mut self, node: &Value) -> Result<CteStatement> {
        fields(node, &["name", "columns", "materialized", "statement"])?;
        if !node["materialized"].is_null() {
            return Err(error(
                "unsupported.cte_materialization",
                "Explicit CTE materialization policy",
            ));
        }
        let name = text(node, "name")?.to_owned();
        let statement = Box::new(self.statement_node(&node["statement"])?);
        let schema = crate::cte::schema(&statement)?.to_vec();
        let table_id = format!("\0sqlolla.cte:{}:{name}", self.work.get());
        let relation = Plan {
            schema,
            node: RelNode::Scan {
                table: table_id.clone(),
            },
        };
        let relation = source::rename(relation, &name, None, array(node, "columns")?)?;
        self.ctes.insert(name.clone(), relation);
        Ok(CteStatement {
            name,
            table_id,
            statement,
        })
    }
}

/// Only lock-bearing lexical query CTEs need the effectful materialization path.
fn locking_ctes(node: &Value) -> Result<bool> {
    for cte in array(node, "ctes")? {
        if !array(&cte["query"], "locking")?.is_empty()
            || volatile_call(&cte["query"])
            || locking_ctes(&cte["query"])?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A volatile query CTE must be materialized once rather than duplicated by lexical inlining.
fn volatile_call(node: &Value) -> bool {
    if node["tag"] == "call"
        && node["name"].as_array().is_some_and(|name| {
            name.last().and_then(Value::as_str).is_some_and(|name| {
                crate::function_registry::builtin(name)
                    .is_some_and(|spec| spec.effect != crate::function_registry::Effect::None)
            })
        })
    {
        return true;
    }
    match node {
        Value::Array(values) => values.iter().any(volatile_call),
        Value::Object(values) => values.values().any(volatile_call),
        _ => false,
    }
}
