//! Stable session expressions require explicit execution-context evidence.
use super::*;

impl Expressions<'_> {
    /// Bind SQL context keywords without obtaining a clock, role or schema implicitly.
    pub(super) fn context_value(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "value"])?;
        let name = text(node, "value")?;
        let (key, ty) = match name {
            "current_timestamp" => ("transaction_timestamp", SqlType::Timestamptz),
            "current_schema" | "current_catalog" | "current_user" | "user" | "current_role"
            | "session_user" => (name, SqlType::Name),
            _ => return Err(error("unsupported.context_value", name)),
        };
        Ok(value(key, ty, name == "current_schema"))
    }
}

/// Resolve only admitted stable builtins; immutable functions retain their scalar path.
pub(super) fn call(name: &str, args: &[BoundExpr]) -> Result<Option<BoundExpr>> {
    if args.is_empty() {
        let result = match name {
            "now" | "transaction_timestamp" => {
                Some(value("transaction_timestamp", SqlType::Timestamptz, false))
            }
            "version" => Some(value("version", SqlType::Text, false)),
            "current_schema" => Some(value(name, SqlType::Name, true)),
            "current_database" => Some(value(name, SqlType::Name, false)),
            "pg_backend_pid" => Some(value(name, SqlType::Int32, false)),
            _ => None,
        };
        if result.is_some() {
            return Ok(result);
        }
    }
    if name == "current_setting" {
        return setting(args).map(Some);
    }
    if crate::catalog_lookup::requires_context(name) {
        return contextual_call(name, args).map(Some);
    }
    Ok(None)
}

/// Bind stable lookup and effectful signatures without pretending they are immutable scalars.
fn contextual_call(name: &str, args: &[BoundExpr]) -> Result<BoundExpr> {
    let (ty, args) = if name == "to_regclass" {
        let [arg] = args else {
            return Err(error("sql.42883", "to_regclass arity"));
        };
        if !matches!(
            arg.ty,
            SqlType::Text
                | SqlType::Name
                | SqlType::Varchar { .. }
                | SqlType::Char { .. }
                | SqlType::Bpchar
        ) {
            return Err(error("sql.42883", "to_regclass requires text"));
        }
        (SqlType::Regclass, vec![argument(arg, SqlType::Text)])
    } else {
        (SqlType::Bool, backend_arguments(args)?)
    };
    Ok(BoundExpr {
        ty,
        nullable: true,
        node: ExprNode::Call {
            name: name.into(),
            args,
        },
    })
}

/// PostgreSQL backend signaling resolves integer PID and optional bigint timeout arguments.
fn backend_arguments(args: &[BoundExpr]) -> Result<Vec<BoundExpr>> {
    if !(1..=2).contains(&args.len()) {
        return Err(error("sql.42883", "pg_terminate_backend arity"));
    }
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let target = if index == 0 {
                SqlType::Int32
            } else {
                SqlType::Int64
            };
            if matches!(arg.node, ExprNode::Literal(Cell::Null(_))) {
                return Ok(literal(Cell::Null(target)));
            }
            if !matches!(arg.ty, SqlType::Int16 | SqlType::Int32)
                && !(index == 1 && arg.ty == SqlType::Int64)
            {
                return Err(error("sql.42883", "pg_terminate_backend signature"));
            }
            Ok(argument(arg, target))
        })
        .collect()
}

/// Signature coercion retains NULL and defers actual value conversion to normal scalar execution.
fn argument(arg: &BoundExpr, ty: SqlType) -> BoundExpr {
    if arg.ty == ty {
        return arg.clone();
    }
    BoundExpr {
        ty,
        nullable: arg.nullable,
        node: ExprNode::Cast {
            arg: Box::new(arg.clone()),
        },
    }
}

/// Setting absence is explicit NULL evidence, distinct from omitted modeling context.
fn setting(args: &[BoundExpr]) -> Result<BoundExpr> {
    let Some(first) = args.first() else {
        return Err(error("sql.42883", "current_setting arity"));
    };
    let ExprNode::Literal(Cell::Text(key)) = &first.node else {
        return Err(error("unsupported.context_key", "Dynamic setting key"));
    };
    let missing_ok = match args {
        [_] => false,
        [_, BoundExpr {
            node: ExprNode::Literal(Cell::Bool(value)),
            ..
        }] => *value,
        _ => return Err(error("unsupported.context_key", "Dynamic setting policy")),
    };
    Ok(value(&format!("setting:{key}"), SqlType::Text, missing_ok))
}

/// Construct a stable typed reference; it cannot be folded as an immutable constant.
fn value(key: &str, ty: SqlType, missing_ok: bool) -> BoundExpr {
    BoundExpr {
        ty,
        nullable: missing_ok,
        node: ExprNode::Context {
            key: key.into(),
            missing_ok,
        },
    }
}
