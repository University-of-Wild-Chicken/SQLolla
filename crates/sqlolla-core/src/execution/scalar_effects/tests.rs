//! Branch-state regressions distinguish inherited prefixes from invocation-local deltas.
use super::*;

/// One exclusive uncalled sequence makes accidental state sharing or reallocation observable.
fn context() -> ExecutionContext {
    serde_json::from_value(
        serde_json::json!({"max_rows":256,"max_outcomes":256,"settings":{},
        "sequences":{"complete":true,"exclusive_snapshot":true,"sequences":{"s":{
            "oid":42,"schema":"public","name":"s","state":{"last_value":41,"is_called":false},
            "increment":3,"minimum":1,"maximum":100,"cycle":false,"cache":1,"can_use":true}}}}),
    )
    .unwrap()
}

/// Already resolved regclass identity avoids a database or parser dependency in transition tests.
fn next() -> BoundExpr {
    BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Call {
            name: "nextval".into(),
            args: vec![crate::expression::literal(
                crate::scalar::input(&SqlType::Regclass, "42").unwrap(),
            )],
        },
    }
}

/// Output positions use fixed integral metadata independent of expression labels.
fn columns(count: usize) -> Vec<Column> {
    (0..count)
        .map(|index| Column {
            name: index.to_string(),
            ty: SqlType::Int64,
            nullable: false,
            qualifier: None,
            schema: None,
        })
        .collect()
}

/// The singleton unit relation executes one projection occurrence.
fn unit() -> Plan {
    Plan {
        schema: vec![],
        node: RelNode::Values { rows: vec![vec![]] },
    }
}

/// Preserve typed expression positions while changing the relation consumed by the projection.
fn projected(input: Plan, expressions: Vec<BoundExpr>) -> Plan {
    Plan {
        schema: columns(expressions.len()),
        node: RelNode::Project {
            input: Box::new(input),
            expressions,
        },
    }
}

/// Execute the complete public statement API against an empty external database.
fn run(plan: StatementPlan) -> Vec<StatementSuccess> {
    evaluate_statement(&plan, &Database::new(), &context())
        .unwrap()
        .outcomes
        .into_iter()
        .map(|outcome| match outcome {
            StatementOutcome::Succeeded(value) => value,
            StatementOutcome::SqlError(error) => panic!("{error:?}"),
        })
        .collect()
}

/// The second projection must see its child's allocation before invoking the same sequence.
#[test]
fn nested_projection_passes_prefix_before_child() {
    let plan = projected(projected(unit(), vec![next()]), vec![next()]);
    let outcomes = run(StatementPlan::Query(plan));
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].rows, vec![vec![Cell::Int64(44)]]);
    assert_eq!(outcomes[0].effects.len(), 2);
}

/// Scalar positions advance one branch's state without mutating the supplied initial snapshot.
#[test]
fn tuple_positions_advance_one_branch() {
    let outcomes = run(StatementPlan::Query(projected(
        unit(),
        vec![next(), next()],
    )));
    assert_eq!(
        outcomes[0].rows,
        vec![vec![Cell::Int64(41), Cell::Int64(44)]]
    );
    assert_eq!(outcomes[0].effects.len(), 2);
}

/// An unselected CASE result cannot allocate or influence the following result position.
#[test]
fn lazy_case_suppresses_allocation() {
    let expr = BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Case {
            branches: vec![(crate::expression::literal(Cell::Bool(false)), next())],
            otherwise: Box::new(crate::expression::literal(Cell::Int64(0))),
        },
    };
    let outcomes = run(StatementPlan::Query(projected(unit(), vec![expr, next()])));
    assert_eq!(
        outcomes[0].rows,
        vec![vec![Cell::Int64(0), Cell::Int64(41)]]
    );
    assert_eq!(outcomes[0].effects.len(), 1);
}

/// Sibling alternatives restart from the same state, even after another alternative emitted events.
#[test]
fn bag_projection_alternatives_are_independent() {
    let input = Plan {
        schema: columns(1),
        node: RelNode::Values {
            rows: vec![vec![Cell::Int64(1)], vec![Cell::Int64(2)]],
        },
    };
    let expr = BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Column(0),
    };
    let outcomes = run(StatementPlan::Query(projected(input, vec![expr, next()])));
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        assert_eq!(
            outcome
                .effects
                .iter()
                .filter_map(|event| match event {
                    EffectEvent::SequenceValue { value, .. } => Some(*value),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![41, 44]
        );
    }
    assert_ne!(outcomes[0].rows, outcomes[1].rows);
}

/// Independent materialized read CTEs expose both dependency-valid sequence assignments.
#[test]
fn independent_ctes_do_not_assume_lexical_order() {
    let outcomes = run(independent_ctes());
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes
        .iter()
        .any(|value| value.rows == vec![vec![Cell::Int64(41), Cell::Int64(44)]]));
    assert!(outcomes
        .iter()
        .any(|value| value.rows == vec![vec![Cell::Int64(44), Cell::Int64(41)]]));
    assert!(outcomes.iter().all(|value| value.effects.len() == 2));
}

/// Whole-job schedules cannot claim completeness for independently streamed multirow producers.
#[test]
fn competing_multirow_ctes_require_tuple_scheduling() {
    let StatementPlan::With(mut plan) = independent_ctes() else {
        unreachable!()
    };
    let source = Plan {
        schema: vec![],
        node: RelNode::Values {
            rows: vec![vec![], vec![]],
        },
    };
    for binding in &mut plan.bindings {
        *binding.statement = StatementPlan::Query(projected(source.clone(), vec![next()]));
    }
    let failure =
        evaluate_statement(&StatementPlan::With(plan), &Database::new(), &context()).unwrap_err();
    assert_eq!(failure.code, "unsupported.cte_tuple_schedule");
}

/// Nested producers and consumers must not allocate all child rows before all parent rows.
#[test]
fn competing_multirow_projections_require_interleaving() {
    let input = Plan {
        schema: vec![],
        node: RelNode::Values {
            rows: vec![vec![], vec![]],
        },
    };
    let plan = projected(projected(input, vec![next()]), vec![next()]);
    let failure =
        evaluate_statement(&StatementPlan::Query(plan), &Database::new(), &context()).unwrap_err();
    assert_eq!(failure.code, "unsupported.projection_effect_interleaving");
}

/// Two materialized scalar jobs are independent predecessors of the final Cartesian read.
fn independent_ctes() -> StatementPlan {
    let bindings = ["a", "b"]
        .map(|name| CteStatement {
            name: name.into(),
            table_id: name.into(),
            statement: Box::new(StatementPlan::Query(projected(unit(), vec![next()]))),
        })
        .to_vec();
    let scan = |name: &str| Plan {
        schema: columns(1),
        node: RelNode::Scan { table: name.into() },
    };
    let body = Plan {
        schema: columns(2),
        node: RelNode::Product {
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        },
    };
    StatementPlan::With(WithPlan {
        bindings,
        body: Box::new(StatementPlan::Query(body)),
    })
}
