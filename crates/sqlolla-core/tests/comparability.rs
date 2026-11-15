//! Missing JSON operators are planning requirements for relations, with scalar NULL bypass retained.
use serde_json::json;
use sqlolla_core::*;

/// Bind a complete parser envelope against an arbitrary typed nullable source column.
fn bind_select(ty: SqlType, distinct: bool) -> StatementPlan {
    let catalog: Catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{
        "t":{"schema":"public","name":"t","columns":[{"name":"x","ty":ty,"nullable":true}]}}}))
    .unwrap();
    let report = json!({"protocol":"sqlolla.parser.v1","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "result":{"parse_status":"parsed","export_status":"exported","ast":{"tag":"select",
        "items":[{"tag":"expression","value":{"tag":"column","name":"x","qualifier":null,"schema":null},"alias":null}],
        "source":{"tag":"table","name":"t","schema":null,"alias":null,"columns":[]},"where":null,"distinct":distinct}}});
    bind_statement(&report, &catalog, &Parameters::new()).unwrap()
}

/// Empty inputs cannot hide absent equality operators, including recursive array elements.
#[test]
fn distinct_json_rejected_before_execution() {
    for ty in [SqlType::Json, SqlType::Array(Box::new(SqlType::Json))] {
        let StatementPlan::Rejected { failure } = bind_select(ty, true) else {
            panic!("Missing comparator must reject during planning");
        };
        assert_eq!(failure.sqlstate, "42883");
        assert_eq!(failure.phase, FailurePhase::Planning);
    }
}

/// JSON projection does not require comparison, while JSONB supports DISTINCT.
#[test]
fn no_unrequested_comparison_requirement() {
    assert!(matches!(
        bind_select(SqlType::Json, false),
        StatementPlan::Query(_)
    ));
    assert!(matches!(
        bind_select(SqlType::Jsonb, true),
        StatementPlan::Query(_)
    ));
}

/// A non-NULL empty array still invokes the generic comparator and checks element support.
#[test]
fn empty_json_array_cannot_use_transport_equality() {
    let array = scalar::input(&SqlType::Array(Box::new(SqlType::Json)), "{}").unwrap();
    assert_eq!(
        scalar::compare_sql(&array, &array).unwrap_err().code,
        "sql.42883"
    );
}

/// SQL strictness bypasses a generic array comparator when either operand is NULL.
#[test]
fn null_array_comparison_remains_unknown() {
    let operand = BoundExpr {
        ty: SqlType::Array(Box::new(SqlType::Json)),
        nullable: true,
        node: ExprNode::Literal(Cell::Null(SqlType::Array(Box::new(SqlType::Json)))),
    };
    let expression = BoundExpr {
        ty: SqlType::Bool,
        nullable: true,
        node: ExprNode::Binary {
            op: BinaryOp::Eq,
            left: Box::new(operand.clone()),
            right: Box::new(operand),
        },
    };
    assert_eq!(
        eval_expr(&expression, &vec![]).unwrap(),
        Cell::Null(SqlType::Bool)
    );
}
