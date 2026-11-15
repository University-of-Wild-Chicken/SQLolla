//! Scoped subquery regressions retain cardinality, NULL truth, lazy branches and refusals.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Build a complete query syntax wrapper without parsing SQL in the computational core.
fn query(body: Value) -> Value {
    json!({"tag":"query","body":body,"ctes":[],"recursive":false,"order_by":[],
        "limit":null,"offset":null,"offset_first":false,"locking":[]})
}

/// Pin parser identity and keep syntax assurance distinct from semantics.
fn report(query: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{
        "parse_status":"parsed","export_status":"exported","ast":query}})
}

/// One nullable integer table makes empty and duplicate scalar results independently testable.
fn catalog() -> Catalog {
    serde_json::from_value(
        json!({"search_path":["public"],"tables":{"t":{"schema":"public","name":"t",
        "columns":[{"name":"k","ty":"int32","nullable":true}]}}}),
    )
    .unwrap()
}

/// Preserve qualification exactly when constructing correlation references.
fn column(qualifier: Option<&str>) -> Value {
    json!({"tag":"column","name":"k","qualifier":qualifier,"schema":null})
}

/// Numbered SQL integer syntax retains its original magnitude string.
fn integer(value: u32) -> Value {
    json!({"tag":"integer","magnitude":value.to_string()})
}

/// Explicit aliases hide the original table qualifier inside nested scopes.
fn table(alias: Option<&str>) -> Value {
    json!({"tag":"table","name":"t","alias":alias,"columns":[],"schema":null})
}

/// Construct one-expression SELECT with every query-body modifier present.
fn select(value: Value, source: Value, predicate: Value) -> Value {
    json!({"tag":"select","items":[{"tag":"expression","value":value,"alias":null}],
        "source":source,"where":predicate,"distinct":false,"group_by":[],"having":null,"distinct_on":[]})
}

/// Wrap one expression in a standalone SELECT.
fn outer(value: Value) -> Value {
    query(select(value, Value::Null, Value::Null))
}

/// A single named query expression retains the complete child syntax.
fn child(tag: &str, value: Value) -> Value {
    json!({"tag":tag,"query":value})
}

/// Bind once and execute against an explicit immutable database snapshot.
fn run(ast: Value, cells: Vec<Cell>) -> Result<OutcomeSet> {
    let plan = bind_statement(&report(ast), &catalog(), &Parameters::new())?;
    let database = Database::from([(
        "t".into(),
        cells.into_iter().map(|cell| vec![cell]).collect(),
    )]);
    evaluate_statement(&plan, &database, &ExecutionContext::default())
}

/// Deterministic fixtures must produce one successful outcome, never an implicit first choice.
fn rows(outcomes: Result<OutcomeSet>) -> Bag {
    let mut values = outcomes.unwrap().outcomes;
    assert_eq!(values.len(), 1);
    match values.pop().unwrap() {
        StatementOutcome::Succeeded(value) => value.rows,
        other => panic!("Expected successful outcome: {other:?}"),
    }
}

/// SQL cardinality errors remain distinct from unsupported execution refusals.
fn sqlstate(outcomes: Result<OutcomeSet>) -> String {
    match outcomes.unwrap().outcomes.into_iter().next().unwrap() {
        StatementOutcome::SqlError(failure) => failure.sqlstate,
        other => panic!("Expected SQL error: {other:?}"),
    }
}

/// Zero rows yields typed NULL; two duplicate rows still violate scalar cardinality.
#[test]
fn scalar_cardinality_counts_occurrences() {
    let inner = query(select(column(None), table(None), Value::Null));
    let ast = outer(child("scalar_subquery", inner));
    assert_eq!(
        rows(run(ast.clone(), vec![])),
        vec![vec![Cell::Null(SqlType::Int32)]]
    );
    assert_eq!(
        rows(run(ast.clone(), vec![Cell::Int32(7)])),
        vec![vec![Cell::Int32(7)]]
    );
    assert_eq!(
        sqlstate(run(ast, vec![Cell::Int32(7), Cell::Int32(7)])),
        "21000"
    );
}

/// EXISTS observes rows, not their target values or target arithmetic errors.
#[test]
fn exists_ignores_target_division_and_retains_null_rows() {
    let division = json!({"tag":"binary","op":"divide","left":integer(1),"right":integer(0)});
    let inner = query(select(division, table(None), Value::Null));
    let ast = outer(child("exists", inner));
    assert_eq!(
        rows(run(ast.clone(), vec![])),
        vec![vec![Cell::Bool(false)]]
    );
    assert_eq!(
        rows(run(ast, vec![Cell::Null(SqlType::Int32)])),
        vec![vec![Cell::Bool(true)]]
    );
}

/// ANY and ALL keep their empty identities and return UNKNOWN for undecided NULL comparisons.
#[test]
fn quantified_empty_and_null_truth_tables() {
    for (quantifier, empty) in [("any", false), ("all", true)] {
        let inner = query(select(column(None), table(None), Value::Null));
        let expression = json!({"tag":"quantified_query","quantifier":quantifier,"op":"eq","arg":integer(1),"query":inner});
        let ast = outer(expression);
        assert_eq!(
            rows(run(ast.clone(), vec![])),
            vec![vec![Cell::Bool(empty)]]
        );
        assert_eq!(
            rows(run(ast, vec![Cell::Null(SqlType::Int32)])),
            vec![vec![Cell::Null(SqlType::Bool)]]
        );
    }
}

/// IN is equality ANY, so a match dominates an accompanying NULL row.
#[test]
fn in_query_match_dominates_null() {
    let inner = query(select(column(None), table(None), Value::Null));
    let expression = json!({"tag":"in_query","arg":integer(1),"query":inner});
    assert_eq!(
        rows(run(
            outer(expression),
            vec![Cell::Null(SqlType::Int32), Cell::Int32(1)]
        )),
        vec![vec![Cell::Bool(true)]]
    );
}

/// Scalar subqueries bind their nearest outer row explicitly rather than capturing global state.
#[test]
fn correlated_scalar_and_nested_depth_preserve_outer_positions() {
    let inner = outer(column(Some("t")));
    let middle = outer(child("scalar_subquery", inner));
    let ast = query(select(
        child("scalar_subquery", middle),
        table(None),
        Value::Null,
    ));
    assert_eq!(
        rows(run(ast, vec![Cell::Int32(4), Cell::Int32(9)])),
        vec![vec![Cell::Int32(4)], vec![Cell::Int32(9)]]
    );
}

/// An inner FROM alias supplies the local name before the outer source can match it.
#[test]
fn local_scope_shadows_outer_same_name() {
    let predicate = json!({"tag":"binary","op":"eq","left":column(Some("i")),"right":integer(4)});
    let inner = query(select(column(None), table(Some("i")), predicate));
    let ast = query(select(
        child("scalar_subquery", inner),
        table(None),
        Value::Null,
    ));
    assert_eq!(
        rows(run(ast, vec![Cell::Int32(4), Cell::Int32(9)])),
        vec![vec![Cell::Int32(4)], vec![Cell::Int32(4)]]
    );
}

/// Runtime CASE skips an unselected scalar subquery that would have a cardinality error.
#[test]
fn case_skips_unselected_subquery() {
    let inner = query(select(column(None), table(Some("i")), Value::Null));
    let condition = json!({"tag":"binary","op":"gt","left":column(None),"right":integer(0)});
    let expression = json!({"tag":"case","operand":null,"branches":[{"when":condition,"then":integer(7)}],"else":child("scalar_subquery",inner)});
    let ast = query(select(expression, table(None), Value::Null));
    assert_eq!(
        rows(run(ast, vec![Cell::Int32(1), Cell::Int32(2)])),
        vec![vec![Cell::Int32(7)], vec![Cell::Int32(7)]]
    );
}

/// Differing permitted LIMIT winners cannot be collapsed into a chosen scalar value.
#[test]
fn ambiguous_scalar_alternatives_are_refused() {
    let mut inner = query(select(column(None), table(None), Value::Null));
    inner["limit"] = integer(1);
    let ast = outer(child("scalar_subquery", inner));
    assert_eq!(
        run(ast, vec![Cell::Int32(1), Cell::Int32(2)])
            .unwrap_err()
            .code,
        "unsupported.subquery_alternatives"
    );
}

/// Ordered ARRAY subqueries retain element order; unordered distinct elements are refused.
#[test]
fn array_query_requires_complete_element_order() {
    let mut inner = query(select(column(None), table(None), Value::Null));
    let unordered = outer(child("array_query", inner.clone()));
    assert_eq!(
        run(unordered, vec![Cell::Int32(1), Cell::Int32(2)])
            .unwrap_err()
            .code,
        "unsupported.subquery_alternatives"
    );
    inner["order_by"] = json!([{"expression":column(None),"direction":"desc","nulls":null}]);
    let result = rows(run(
        outer(child("array_query", inner)),
        vec![Cell::Int32(1), Cell::Int32(2)],
    ));
    assert_eq!(scalar::output(&result[0][0]).unwrap(), "{2,1}");
}

/// Child runtime errors are refused when PostgreSQL streaming may change which error is seen.
#[test]
fn child_runtime_error_precedence_fails_closed() {
    let division = json!({"tag":"binary","op":"divide","left":integer(1),"right":column(None)});
    let inner = query(select(division, table(None), Value::Null));
    assert_eq!(
        run(outer(child("scalar_subquery", inner)), vec![Cell::Int32(0)])
            .unwrap_err()
            .code,
        "unsupported.subquery_error_order"
    );
}

/// Correlated evaluation consumes shared fuel instead of resetting its budget per nested query.
#[test]
fn correlated_expansion_exhausts_statement_budget() {
    let predicate =
        json!({"tag":"binary","op":"eq","left":column(Some("i")),"right":column(Some("t"))});
    let inner = query(select(integer(1), table(Some("i")), predicate));
    let ast = query(select(child("exists", inner), table(None), Value::Null));
    let input = (0..256).map(Cell::Int32).collect();
    assert_eq!(
        run(ast, input).unwrap_err().code,
        "resource.execution_steps"
    );
}

/// A decisive runtime Boolean operand suppresses an unreachable scalar cardinality failure.
#[test]
fn boolean_short_circuit_skips_subquery() {
    let inner = query(select(column(None), table(Some("i")), Value::Null));
    let left = json!({"tag":"binary","op":"lt","left":column(None),"right":integer(0)});
    let right =
        json!({"tag":"binary","op":"gt","left":child("scalar_subquery",inner),"right":integer(0)});
    let expression = json!({"tag":"binary","op":"and","left":left,"right":right});
    let ast = query(select(expression, table(None), Value::Null));
    assert_eq!(
        rows(run(ast, vec![Cell::Int32(1), Cell::Int32(2)])),
        vec![vec![Cell::Bool(false)], vec![Cell::Bool(false)]]
    );
}

/// Stable context inside a nested query is read from the same explicit statement snapshot.
#[test]
fn subquery_context_uses_explicit_snapshot() {
    let inner = outer(json!({"tag":"context_value","value":"current_timestamp"}));
    let plan = bind_statement(
        &report(outer(child("scalar_subquery", inner))),
        &catalog(),
        &Parameters::new(),
    )
    .unwrap();
    let timestamp = scalar::input(&SqlType::Timestamptz, "2024-01-02 03:04:05+00").unwrap();
    let mut context = ExecutionContext::default();
    context
        .settings
        .insert("transaction_timestamp".into(), timestamp.clone());
    let result = evaluate_statement(&plan, &Database::from([("t".into(), vec![])]), &context);
    assert_eq!(rows(result), vec![vec![timestamp]]);
}

/// Outer-only aggregate arguments can belong to a different SQL query level and remain refused.
#[test]
fn outer_aggregate_query_level_is_not_guessed() {
    let sum = json!({"tag":"aggregate","fn":"sum","arg":column(Some("t")),"distinct":false});
    let inner = outer(sum);
    let ast = query(select(
        child("scalar_subquery", inner),
        table(None),
        Value::Null,
    ));
    assert_eq!(
        run(ast, vec![Cell::Int32(1), Cell::Int32(2)])
            .unwrap_err()
            .code,
        "unsupported.outer_aggregate_scope"
    );
}

/// PostgreSQL result labels survive scalar-query and EXISTS/ARRAY expression boundaries.
#[test]
fn subquery_output_labels_are_bound_metadata() {
    let values = [
        (child("exists", outer(integer(1))), "exists"),
        (child("array_query", outer(integer(1))), "array"),
        (
            child(
                "scalar_subquery",
                query(select(column(None), table(None), Value::Null)),
            ),
            "k",
        ),
    ];
    for (expression, expected) in values {
        let plan =
            bind_statement(&report(outer(expression)), &catalog(), &Parameters::new()).unwrap();
        let StatementPlan::Query(plan) = plan else {
            panic!("Expected query");
        };
        assert_eq!(plan.schema[0].name, expected);
    }
}

/// Weak cast and CASE names do not replace original column names or leak through an outer cast.
#[test]
fn case_cast_labels_respect_name_strength() {
    let cast = |arg| {
        json!({"tag":"cast","arg":arg,
        "type":{"name":{"tag":"scalar_type","name":"bigint"},"modifiers":[]}})
    };
    let case = |otherwise| {
        json!({"tag":"case","operand":null,
        "branches":[{"when":{"tag":"boolean","value":false},"then":integer(7)}],"else":otherwise})
    };
    let examples = [
        (case(cast(integer(7))), "case"),
        (cast(case(integer(7))), "int8"),
        (case(cast(column(None))), "k"),
        (cast(case(column(None))), "k"),
    ];
    for (expression, expected) in examples {
        let ast = query(select(expression, table(None), Value::Null));
        let plan = bind_statement(&report(ast), &catalog(), &Parameters::new()).unwrap();
        let StatementPlan::Query(plan) = plan else {
            panic!("Expected query")
        };
        assert_eq!(plan.schema[0].name, expected);
    }
}
