//! Semantic regressions for explicit order, ties, pagination and query boundaries.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Construct a pinned complete syntax envelope without parsing SQL in Rust.
fn report(body: Value, order: Vec<Value>, limit: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed",
        "export_status":"exported","ast":{"tag":"query","body":body,"ctes":[],"recursive":false,
        "order_by":order,"limit":limit,"offset":null,"offset_first":false,"locking":[]}}})
}

/// Two columns deliberately allow an ordering key absent from the result projection.
fn catalog() -> Catalog {
    serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[
            {"name":"id","ty":"int32","nullable":true},
            {"name":"name","ty":"text","nullable":false}]}}}))
    .unwrap()
}

/// Build one unresolved parser column reference.
fn column(name: &str) -> Value {
    json!({"tag":"column","name":name,"qualifier":null,"schema":null})
}

/// SELECT one physical column from the fixture table.
fn select(name: &str) -> Value {
    json!({"tag":"select","items":[{"tag":"expression","value":column(name),"alias":null}],
        "source":{"tag":"table","name":"t","alias":null,"columns":[],"schema":null},
        "where":null,"distinct":false,"group_by":[],"having":null,"distinct_on":[]})
}

/// Ascending ordering defaults to NULLS LAST.
fn order(name: &str) -> Value {
    json!({"expression":column(name),"direction":"asc","nulls":null})
}

/// Bind and completely evaluate one query with typed fixture data.
fn run(ast: Value, rows: Bag) -> OutcomeSet {
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    evaluate_statement(
        &plan,
        &Database::from([("t".into(), rows)]),
        &ExecutionContext::default(),
    )
    .unwrap()
}

/// Extract successful rows while refusing to treat errors as empty query results.
fn results(outcomes: OutcomeSet) -> Vec<(ResultOrdering, Bag)> {
    outcomes
        .outcomes
        .into_iter()
        .map(|outcome| match outcome {
            StatementOutcome::Succeeded(value) => (value.ordering, value.rows),
            other => panic!("Unexpected outcome: {other:?}"),
        })
        .collect()
}

/// ORDER BY may use a source expression not projected by a nondistinct SELECT.
#[test]
fn hidden_order_key_is_preserved() {
    let rows = vec![
        vec![Cell::Int32(2), Cell::Text("b".into())],
        vec![Cell::Int32(1), Cell::Text("a".into())],
    ];
    let result = results(run(
        report(select("name"), vec![order("id")], Value::Null),
        rows,
    ));
    assert_eq!(
        result,
        vec![(
            ResultOrdering::Ordered,
            vec![vec![Cell::Text("a".into())], vec![Cell::Text("b".into())]]
        )]
    );
}

/// Ties have multiple permitted pages; no canonical tie winner may stand in for completeness.
#[test]
fn tied_limit_enumerates_both_rows() {
    let rows = vec![
        vec![Cell::Int32(1), Cell::Text("a".into())],
        vec![Cell::Int32(1), Cell::Text("b".into())],
    ];
    let limit = json!({"tag":"integer","magnitude":"1"});
    let result = results(run(report(select("name"), vec![order("id")], limit), rows));
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|(ordering, rows)| *ordering == ResultOrdering::Ordered && rows.len() == 1));
    assert_ne!(result[0].1, result[1].1);
}

/// Without ORDER BY every one-row subbag is a possible LIMIT result.
#[test]
fn unordered_limit_enumerates_subbags() {
    let rows = vec![
        vec![Cell::Int32(1), Cell::Text("a".into())],
        vec![Cell::Int32(2), Cell::Text("b".into())],
    ];
    let limit = json!({"tag":"integer","magnitude":"1"});
    let ast = report(select("id"), vec![], limit);
    let result = results(run(ast.clone(), rows.clone()));
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|(ordering, _)| *ordering == ResultOrdering::Bag));
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    let context = ExecutionContext {
        max_outcomes: 1,
        ..Default::default()
    };
    assert_eq!(
        evaluate_statement(&plan, &Database::from([("t".into(), rows)]), &context)
            .unwrap_err()
            .code,
        "resource.outcome_count"
    );
}

/// DISTINCT forbids ordering by an unselected expression.
#[test]
fn distinct_rejects_hidden_key() {
    let mut body = select("name");
    body["distinct"] = json!(true);
    let plan = bind_statement(
        &report(body, vec![order("id")], Value::Null),
        &catalog(),
        &Parameters::new(),
    )
    .unwrap();
    let StatementPlan::Rejected { failure } = plan else {
        panic!("Expected SQL error");
    };
    assert_eq!(failure.sqlstate, "42P10");
}

/// SQL NULL ordering is explicit rather than inherited from Rust enum ordering.
#[test]
fn nulls_follow_direction_defaults() {
    let rows = vec![
        vec![Cell::Null(SqlType::Int32), Cell::Text("null".into())],
        vec![Cell::Int32(1), Cell::Text("one".into())],
    ];
    let ascending = results(run(
        report(select("name"), vec![order("id")], Value::Null),
        rows.clone(),
    ));
    assert_eq!(ascending[0].1[0], vec![Cell::Text("one".into())]);
    let mut descending = order("id");
    descending["direction"] = json!("desc");
    let descending = results(run(
        report(select("name"), vec![descending], Value::Null),
        rows,
    ));
    assert_eq!(descending[0].1[0], vec![Cell::Text("null".into())]);
}

/// Bound bigint pagination parameters retain their declared type and resolve deterministically.
#[test]
fn bigint_limit_parameter_is_supported() {
    let ast = report(
        select("id"),
        vec![],
        json!({"tag":"numbered_parameter","index":1}),
    );
    let parameters = Parameters::from([(1, Cell::Int64(1))]);
    assert!(bind_statement(&ast, &catalog(), &parameters).is_ok());
}

/// Build arithmetic and comparison parser nodes without using a SQL-text parser.
fn binary(op: &str, left: Value, right: Value) -> Value {
    json!({"tag":"binary","op":op,"left":left,"right":right})
}

/// Exact integer spelling preserves parser literal representation.
fn integer(value: i64) -> Value {
    json!({"tag":"integer","magnitude":value.to_string()})
}

/// Select an expression against the fixture table, optionally without FROM.
fn expression_query(value: Value, source: bool) -> Value {
    let mut body = select("id");
    body["items"][0]["value"] = value;
    if !source {
        body["source"] = Value::Null;
    }
    report(body, vec![], Value::Null)
}

/// Planned literal VALUES keep their native representation, exact values and multiplicity.
#[test]
fn literal_values_retain_native_form_and_duplicate_occurrences() {
    let value = binary("add", integer(2), integer(3));
    let ast = report(
        json!({"tag":"values","rows":[[value],[value]]}),
        vec![],
        Value::Null,
    );
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    let StatementPlan::Query(plan) = plan else {
        panic!("Expected query")
    };
    let RelNode::Values { rows } = &plan.node else {
        panic!("Expected literal VALUES")
    };
    assert_eq!(rows, &vec![vec![Cell::Int32(5)], vec![Cell::Int32(5)]]);
    assert_eq!(eval(&plan, &Database::new()).unwrap(), rows.clone());
}

/// Stable VALUES cells remain unevaluated until their declared runtime context exists.
#[test]
fn stable_values_are_not_literalized() {
    let value = json!({"tag":"call","name":["now"],"args":[]});
    let ast = report(
        json!({"tag":"values","rows":[[value]]}),
        vec![],
        Value::Null,
    );
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    let StatementPlan::Query(query) = &plan else {
        panic!("Expected query")
    };
    assert!(matches!(query.node, RelNode::EvaluateValues { .. }));
    assert_eq!(
        evaluate_statement(&plan, &Database::new(), &ExecutionContext::default())
            .unwrap_err()
            .code,
        "unsupported.context_value"
    );
}

/// Constant arithmetic is planned even when an empty input produces no runtime rows.
#[test]
fn constant_errors_precede_empty_input_execution() {
    let query = expression_query(binary("divide", integer(1), integer(0)), true);
    let outcomes = run(query, vec![]);
    let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
        panic!("Expected SQL error");
    };
    assert_eq!(failure.sqlstate, "22012");
    assert_eq!(failure.phase, FailurePhase::Planning);
}

/// Variable CASE and COALESCE arms still expose immutable planning errors.
#[test]
fn reachable_constant_branches_fail_during_planning() {
    let bad = binary("divide", integer(1), integer(0));
    let case = json!({"tag":"case","operand":null,"branches":[{
        "when":binary("eq",column("id"),integer(0)),"then":bad}],"else":integer(0)});
    let coalesce = json!({"tag":"coalesce","args":[column("id"),bad]});
    for expr in [case, coalesce] {
        let outcomes = run(
            expression_query(expr, true),
            vec![vec![Cell::Int32(1), Cell::Text("x".into())]],
        );
        let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
            panic!("Expected error");
        };
        assert_eq!(failure.phase, FailurePhase::Planning);
        assert_eq!(failure.sqlstate, "22012");
    }
}

/// Statically dead Boolean/CASE arms are type-checked but not constant-evaluated.
#[test]
fn decisive_constants_discard_unreachable_errors() {
    let bad = binary("divide", integer(1), integer(0));
    let false_value = json!({"tag":"boolean","value":false});
    let and = binary(
        "and",
        false_value.clone(),
        binary("eq", bad.clone(), integer(0)),
    );
    assert_eq!(
        results(run(expression_query(and, false), vec![]))[0].1,
        vec![vec![Cell::Bool(false)]]
    );
    let case = json!({"tag":"case","operand":null,"branches":[{"when":false_value,"then":bad}],"else":integer(7)});
    assert_eq!(
        results(run(expression_query(case, false), vec![]))[0].1,
        vec![vec![Cell::Int32(7)]]
    );
}

/// Stable clocks are neither fabricated nor folded into an immutable plan.
#[test]
fn transaction_timestamp_requires_explicit_context() {
    let ast = expression_query(json!({"tag":"call","name":["now"],"args":[]}), false);
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    assert_eq!(
        evaluate_statement(&plan, &Database::new(), &ExecutionContext::default())
            .unwrap_err()
            .code,
        "unsupported.context_value"
    );
    let clock = scalar::input(&SqlType::Timestamptz, "2026-11-14 12:00:00+00").unwrap();
    let context = ExecutionContext {
        settings: std::collections::BTreeMap::from([(
            "transaction_timestamp".into(),
            clock.clone(),
        )]),
        ..Default::default()
    };
    let outcomes = evaluate_statement(&plan, &Database::new(), &context).unwrap();
    assert_eq!(results(outcomes)[0].1, vec![vec![clock]]);
}

/// ANY/ALL preserve empty identities, NULL arrays and nullable comparison members.
#[test]
fn quantified_array_truth_table() {
    let arrays = [
        ("{}", false, true),
        ("{NULL}", false, false),
        ("{NULL,1}", true, false),
    ];
    for (spelling, any, all) in arrays {
        for (quantifier, expected) in [("any", any), ("all", all)] {
            let ast = json!({"tag":"quantified_comparison","op":"eq","quantifier":quantifier,
                "left":integer(1),"right":{"tag":"numbered_parameter","index":1}});
            let value = scalar::input(&SqlType::Array(Box::new(SqlType::Int32)), spelling).unwrap();
            let rows = parameterized_rows(ast, value);
            let expected =
                if spelling == "{NULL}" || (spelling == "{NULL,1}" && quantifier == "all") {
                    Cell::Null(SqlType::Bool)
                } else {
                    Cell::Bool(expected)
                };
            assert_eq!(rows, vec![vec![expected]]);
        }
    }
}

/// Evaluate a single typed parameter expression without adding table fixtures.
fn parameterized_rows(ast: Value, value: Cell) -> Bag {
    let plan = bind_statement(
        &expression_query(ast, false),
        &catalog(),
        &Parameters::from([(1, value)]),
    )
    .unwrap();
    results(evaluate_statement(&plan, &Database::new(), &ExecutionContext::default()).unwrap())[0]
        .1
        .clone()
}

/// Array construction preserves SQL NULL elements and resolves all-unknown nonempty arrays as text.
#[test]
fn array_constructor_context_is_preserved() {
    let value = json!({"tag":"array","elements":[{"tag":"text","value":"a"},{"tag":"sql_null"}]});
    let result = results(run(expression_query(value, false), vec![]));
    let expected = scalar::input(&SqlType::Array(Box::new(SqlType::Text)), "{a,NULL}").unwrap();
    assert_eq!(result[0].1, vec![vec![expected]]);
}

/// Row equality can decide a mismatch despite another NULL; ordering stops at a NULL pair.
#[test]
fn row_comparison_null_stopping_rules() {
    let row = |values: Vec<Value>| json!({"tag":"row","elements":values});
    let left = row(vec![json!({"tag":"sql_null"}), integer(1)]);
    let right = row(vec![integer(2), integer(2)]);
    for (op, expected) in [
        ("eq", Cell::Bool(false)),
        ("ne", Cell::Bool(true)),
        ("lt", Cell::Null(SqlType::Bool)),
    ] {
        let ast = binary(op, left.clone(), right.clone());
        assert_eq!(
            results(run(expression_query(ast, false), vec![]))[0].1,
            vec![vec![expected]]
        );
    }
    let ast = binary(
        "lt",
        row(vec![integer(1), json!({"tag":"sql_null"})]),
        row(vec![integer(2), integer(0)]),
    );
    assert_eq!(
        results(run(expression_query(ast, false), vec![]))[0].1,
        vec![vec![Cell::Bool(true)]]
    );
}

/// LIKE treats underscore as one Unicode character and escaped percent as literal text.
#[test]
fn like_tokens_and_escape_are_sql_specific() {
    for (value, pattern, expected) in [
        ("éx", "_x", true),
        ("a%b", r"a\%b", true),
        ("ab", "a_b", false),
        ("abcdef", "a%f", true),
    ] {
        let ast = json!({"tag":"pattern","arg":{"tag":"text","value":value},
            "pattern":{"tag":"text","value":pattern},"insensitive":false});
        assert_eq!(
            results(run(expression_query(ast, false), vec![]))[0].1,
            vec![vec![Cell::Bool(expected)]]
        );
    }
    let ast = json!({"tag":"pattern","arg":{"tag":"text","value":"x"},
        "pattern":{"tag":"text","value":"x\\"},"insensitive":false});
    let outcomes = run(expression_query(ast, false), vec![]);
    let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
        panic!("Expected trailing-escape SQL error");
    };
    assert_eq!(failure.sqlstate, "22025");
}
