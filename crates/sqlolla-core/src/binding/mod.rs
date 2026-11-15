//! Binding entry point and query-family dispatch; no SQL text is parsed here.
mod budget;
mod conflict;
pub(crate) mod ddl;
mod defaults;
pub use ddl::bind_statement_with_source;
mod distinct_on;
mod effects;
mod extended;
mod grouping;
mod initialization;
mod mutation;
mod nominal;
mod normalize;
mod projection;
mod returning;
mod set_returning;
mod source;
pub(crate) mod subquery;
mod values;
mod window;
mod with_statement;
use crate::expression::{common, Expressions};
use crate::json::{array, boolean, bounded, error, fields, text};
use crate::*;
use serde_json::Value;
use std::collections::BTreeMap;

/// Pure immutable binding context; CTEs have lexical visibility.
#[derive(Clone)]
struct Binder<'a> {
    catalog: &'a Catalog,
    params: &'a Parameters,
    ctes: BTreeMap<String, Plan>,
    work: std::rc::Rc<std::cell::Cell<usize>>,
    extended: bool,
    outer: Vec<Vec<Column>>,
    view_uses: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<String>>>,
    table_uses: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<String>>>,
    resolved_relations: Option<std::collections::BTreeSet<(String, String)>>,
    initial_relations: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<String>>>,
    locking_seen: std::rc::Rc<std::cell::Cell<bool>>,
}

/// Bind a pinned versioned SQLeanParser report into the admitted typed algebra.
/// Refuses plans beyond 256 expanded relation/expression nodes, 64 columns,
/// or 10,000 query/source/type-inference visits before CTE growth becomes excessive.
pub fn bind(report: &Value, catalog: &Catalog, params: &Parameters) -> Result<Plan> {
    validate_parameters(params)?;
    let ast = legacy_ast(report)?;
    source::validate_catalog(catalog)?;
    binder(catalog, params, false).query(ast, None)
}

/// Validate the unchanged legacy envelope without executing its semantic binder.
fn legacy_ast(report: &Value) -> Result<&Value> {
    bounded(report, 0, &mut 20_000)?;
    if text(report, "protocol")? != "sqlolla.parser.v1" {
        return Err(error("unsupported.protocol", "Expected sqlolla.parser.v1"));
    }
    if text(report, "parser_commit")? != "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb" {
        return Err(error(
            "unsupported.parser_version",
            "Parser commit mismatch",
        ));
    }
    let result = &report["result"];
    if text(result, "parse_status")? != "parsed" || text(result, "export_status")? != "exported" {
        return Err(error(
            "unsupported.parser_result",
            "Parser did not export an admitted AST",
        ));
    }
    Ok(&result["ast"])
}

/// Construct an isolated lexical binder with a shared deterministic work budget.
fn binder<'a>(catalog: &'a Catalog, params: &'a Parameters, extended: bool) -> Binder<'a> {
    Binder {
        catalog,
        params,
        ctes: BTreeMap::new(),
        work: std::rc::Rc::new(std::cell::Cell::new(10_000)),
        extended,
        outer: Vec::new(),
        view_uses: Default::default(),
        table_uses: Default::default(),
        resolved_relations: None,
        initial_relations: Default::default(),
        locking_seen: Default::default(),
    }
}

/// Bind either pinned parser protocol into an explicit query or mutation statement.
pub fn bind_statement(
    report: &Value,
    catalog: &Catalog,
    params: &Parameters,
) -> Result<StatementPlan> {
    validate_parameters(params)?;
    let scope = binder(catalog, params, true);
    let result = bind_statement_checked(report, &scope);
    scope.initialized_statement(result)
}

/// Validate even unused parameter cells before SQL planning can produce an unrelated verdict.
pub(crate) fn validate_parameters(params: &Parameters) -> Result<()> {
    params.values().try_for_each(Cell::validate)
}

/// Preserve parser/admission failures while binding both supported protocol versions.
fn bind_statement_checked(report: &Value, scope: &Binder<'_>) -> Result<StatementPlan> {
    if text(report, "protocol")? == "sqlolla.parser.v1" {
        source::validate_catalog(scope.catalog)?;
        return scope
            .query(legacy_ast(report)?, None)
            .map(StatementPlan::Query);
    }
    normalize::validate_report(report)?;
    source::validate_catalog(scope.catalog)?;
    let node = &report["result"]["ast"];
    scope.statement_node(node)
}

/// Only explicitly classified SQL failures become rejected statement plans.
fn rejected_statement(failure: Error) -> Result<StatementPlan> {
    if let Some(sql) = failure.sql {
        return Ok(StatementPlan::Rejected { failure: *sql });
    }
    let Some(code) = failure.code.strip_prefix("sql.") else {
        return Err(failure);
    };
    let phase = FailurePhase::Unspecified;
    Ok(StatementPlan::Rejected {
        failure: SqlFailure {
            sqlstate: code.into(),
            phase,
            constraint_name: None,
            column_name: None,
            exposed_rows: vec![],
            effects: vec![],
        },
    })
}

/// Bind a parser-generated single-expression SELECT against an explicit metadata row scope.
pub fn bind_metadata_expression(report: &Value, columns: &[Column]) -> Result<BoundExpr> {
    bind_metadata_functions(report, columns, None)
}

/// Bind metadata with the exact declared SQL function environment and no relational query scope.
pub fn bind_metadata_functions(
    report: &Value,
    columns: &[Column],
    functions: Option<&SqlFunctions>,
) -> Result<BoundExpr> {
    bind_metadata_context(report, columns, functions, None)
}

/// Bind defaults with catalog identity and function environments while excluding relational reads.
pub fn bind_metadata_context(
    report: &Value,
    columns: &[Column],
    functions: Option<&SqlFunctions>,
    lookup: Option<&CatalogLookup>,
) -> Result<BoundExpr> {
    let value = metadata_expression(report)?;
    let parameters = Parameters::new();
    Expressions {
        lookup,
        columns,
        params: &parameters,
        extended: true,
        scope: None,
        functions,
    }
    .bind(&value, None)
}

/// Preserve the single scalar expression from a strictly validated parser SELECT envelope.
fn metadata_expression(report: &Value) -> Result<Value> {
    let node = if text(report, "protocol")? == "sqlolla.parser.v1" {
        legacy_ast(report)?.clone()
    } else {
        normalize::validate_report(report)?;
        let normalized = normalize::query(&report["result"]["ast"])?;
        metadata_body(&normalized)?.clone()
    };
    fields(&node, &["tag", "items", "source", "where", "distinct"])?;
    if text(&node, "tag")? != "select"
        || !node["source"].is_null()
        || !node["where"].is_null()
        || boolean(&node, "distinct")?
    {
        return Err(error(
            "invalid.metadata_expression",
            "Metadata needs one standalone expression",
        ));
    }
    Ok(metadata_item(&node)?["value"].clone())
}

/// Reject wildcard, multiple-output or malformed metadata projection shapes.
fn metadata_item(node: &Value) -> Result<&Value> {
    let items = array(node, "items")?;
    let [item] = items.as_slice() else {
        return Err(error(
            "invalid.metadata_expression",
            "Expected one expression",
        ));
    };
    fields(item, &["tag", "value", "alias"])?;
    if text(item, "tag")? != "expression" {
        return Err(error(
            "invalid.metadata_expression",
            "Wildcard metadata expression",
        ));
    }
    Ok(item)
}

/// No CTE/order/pagination/locking modifier may hide inside a metadata expression.
fn metadata_body(node: &Value) -> Result<&Value> {
    fields(node, &["tag", "body", "order_by", "limit", "offset"])?;
    if text(node, "tag")? != "modified_query"
        || !array(node, "order_by")?.is_empty()
        || !node["limit"].is_null()
        || !node["offset"].is_null()
    {
        return Err(error(
            "invalid.metadata_expression",
            "Metadata query modifiers are not allowed",
        ));
    }
    Ok(&node["body"])
}

impl Binder<'_> {
    /// Dispatch query nodes while rejecting every unimplemented family.
    fn query(&self, node: &Value, expected: Option<&[SqlType]>) -> Result<Plan> {
        self.charge()?;
        let plan = match text(node, "tag")? {
            "select" => self.select(node, expected),
            "grouped_select" => self.grouped_select(node, &[], expected),
            "distinct_on_select" => self.distinct_on(node, &[], expected),
            "values" => self.values(node, expected),
            "set_operation" => self.set(node),
            "with" => self.with_scope(node)?.query(&node["body"], expected),
            "modified_query" => self.modified_query(node, expected),
            tag => Err(error("unsupported.query", tag)),
        }?;
        budget::check(&plan)?;
        crate::comparison::relation(&plan)?;
        Ok(plan)
    }

    /// Retain unknown SELECT literals until the enclosing set operation resolves them.
    fn hints(&self, node: &Value) -> Result<Vec<Option<SqlType>>> {
        self.charge()?;
        match text(node, "tag")? {
            "select" | "distinct_on_select" => self.select_hints(node),
            "grouped_select" => Ok(self
                .grouped_select(node, &[], None)?
                .schema
                .into_iter()
                .map(|column| Some(column.ty))
                .collect()),
            "values" => Ok(self.value_types(node)?.into_iter().map(Some).collect()),
            "set_operation" => Ok(self.set_types(node)?.into_iter().map(Some).collect()),
            "with" => self.with_scope(node)?.hints(&node["body"]),
            "modified_query" => self.hints(&node["body"]),
            tag => Err(error("unsupported.query", tag)),
        }
    }

    /// Resolve set operands as PostgreSQL does, including nested unknown resolution.
    fn set_types(&self, node: &Value) -> Result<Vec<SqlType>> {
        let (left, right) = (self.hints(&node["left"])?, self.hints(&node["right"])?);
        if left.len() != right.len() {
            return Err(error(
                "invalid.set_arity",
                "Set operands have different widths",
            ));
        }
        left.into_iter()
            .zip(right)
            .map(|(a, b)| common(a, b))
            .collect()
    }

    /// Lower set ALL/DISTINCT with compatible positional result types.
    fn set(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "op", "all", "left", "right"])?;
        let types = self.set_types(node)?;
        let (left, right) = (
            self.query(&node["left"], Some(&types))?,
            self.query(&node["right"], Some(&types))?,
        );
        let op = match text(node, "op")? {
            "union" => SetOp::Union,
            "intersect" => SetOp::Intersect,
            "except" => SetOp::Except,
            other => return Err(error("unsupported.set", other)),
        };
        let mut schema = left.schema.clone();
        for (column, other) in schema.iter_mut().zip(&right.schema) {
            column.nullable |= other.nullable;
        }
        Ok(Plan {
            schema,
            node: RelNode::Set {
                op,
                all: boolean(node, "all")?,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// Resolve nonrecursive CTEs in lexical order without exposing later bindings.
    fn with_scope(&self, node: &Value) -> Result<Self> {
        self.charge()?;
        fields(node, &["tag", "ctes", "body"])?;
        let mut scope = self.clone();
        let mut names = std::collections::BTreeSet::new();
        for cte in array(node, "ctes")? {
            fields(cte, &["name", "columns", "query"])?;
            let name = text(cte, "name")?;
            if !names.insert(name.to_owned()) {
                return Err(error("invalid.cte", "Duplicate CTE name"));
            }
            let plan = scope.query(&cte["query"], None)?;
            let plan = source::rename(plan, name, None, array(cte, "columns")?)?;
            budget::check(&plan)?;
            scope.ctes.insert(name.into(), plan);
        }
        Ok(scope)
    }

    /// Make expression scope explicit for every bind operation.
    fn expressions<'a>(&'a self, columns: &'a [Column]) -> Expressions<'a> {
        Expressions {
            lookup: self.catalog.relation_lookup.as_deref(),
            functions: self.catalog.functions.as_ref(),
            columns,
            params: self.params,
            extended: self.extended,
            scope: Some(self),
        }
    }

    /// Share a deterministic local work budget across cloned lexical CTE scopes.
    fn charge(&self) -> Result<()> {
        let remaining = self
            .work
            .get()
            .checked_sub(1)
            .ok_or_else(|| error("resource.bind_work", "Binding work budget exceeded"))?;
        self.work.set(remaining);
        Ok(())
    }
}
