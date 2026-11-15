//! Total SQL three-valued expressions over finite, exact scalar atoms.
use super::builder::Builder;
use super::{Layout, Property};
use sqlolla_core::{BinaryOp, BoundExpr, Cell, ExprNode};

impl Builder<'_> {
    /// Emit scalar declarations and exact comparison tables; user strings never become Alloy code.
    pub fn prelude(&self) -> String {
        let mut text = "abstract sig V {}\none sig Null extends V {}\n".to_owned();
        for name in self.atoms.values() {
            text.push_str(&format!("one sig {name} extends V {{}}\n"));
        }
        let less = self.integer_order();
        text.push_str(&format!(
            "one sig Ordering {{ lt: V -> V }}\nfact {{ Ordering.lt = {less} }}\n"
        ));
        text.push_str(&self.truth_functions());
        text
    }

    /// Define the finite integer order using host integer comparisons, not bounded Alloy arithmetic.
    fn integer_order(&self) -> String {
        let mut pairs = Vec::new();
        for (left, a) in &self.atoms {
            for (right, b) in &self.atoms {
                if let (Cell::Int32(x), Cell::Int32(y)) = (left, right)
                    && x < y
                {
                    pairs.push(format!("{a}->{b}"));
                }
            }
        }
        if pairs.is_empty() {
            "none->none".into()
        } else {
            pairs.join(" + ")
        }
    }

    /// Preserve UNKNOWN through equality, negation, conjunction and disjunction.
    fn truth_functions(&self) -> String {
        let t = self.atom(&Cell::Bool(true));
        let f = self.atom(&Cell::Bool(false));
        let eq = format!(
            "fun eqv[a,b:V]:one V {{ (a=Null or b=Null) => Null else (a=b => {t} else {f}) }}\n"
        );
        let lt = format!(
            "fun ltv[a,b:V]:one V {{ (a=Null or b=Null) => Null else (a->b in Ordering.lt => {t} else {f}) }}\n"
        );
        let not =
            format!("fun notv[a:V]:one V {{ a=Null => Null else (a={t} => {f} else {t}) }}\n");
        let and = format!(
            "fun andv[a,b:V]:one V {{ (a={f} or b={f}) => {f} else ((a=Null or b=Null) => Null else {t}) }}\n"
        );
        let or = format!(
            "fun orv[a,b:V]:one V {{ (a={t} or b={t}) => {t} else ((a=Null or b=Null) => Null else {f}) }}\n"
        );
        let isnull = format!("fun nullv[a:V]:one V {{ a=Null => {t} else {f} }}\n");
        format!("{eq}{lt}{not}{and}{or}{isnull}")
    }

    /// Translate a typed expression without simplifying away NULL behavior.
    pub fn expression(&self, expression: &BoundExpr, cells: &[String]) -> Result<String, String> {
        match &expression.node {
            ExprNode::Literal(value) => Ok(self.atom(value)),
            ExprNode::Column(index) => cells
                .get(*index)
                .cloned()
                .ok_or("invalid bound column".into()),
            ExprNode::Not(arg) => Ok(format!("notv[{}]", self.expression(arg, cells)?)),
            ExprNode::Binary { op, left, right } => {
                let a = self.expression(left, cells)?;
                let b = self.expression(right, cells)?;
                Ok(binary(*op, &a, &b))
            }
            ExprNode::IsNull { arg, negated } => {
                let value = format!("nullv[{}]", self.expression(arg, cells)?);
                Ok(if *negated {
                    format!("notv[{value}]")
                } else {
                    value
                })
            }
            #[allow(unreachable_patterns)]
            _ => Err("expression requires the explicit outcome encoding profile".into()),
        }
    }

    /// Construct a violation predicate independently of the row semantics facts.
    pub fn property(&self, rule: &Property, output: &Layout) -> Result<String, String> {
        let live = self.live("r");
        match rule {
            Property::NoSqlError => Err("SQL-error properties require the outcome profile".into()),
            Property::MaxRows { max } if *max >= output.rows.len() => Ok("some none".into()),
            Property::MaxRows { max } => Ok(format!("#{{r:{} | {live}}} > {max}", output.name)),
            Property::NoValue { column, value } | Property::OnlyValue { column, value } => {
                let col = output
                    .schema
                    .get(*column)
                    .ok_or("property column outside schema")?;
                if col.ty != super::builder::cell_type(value) {
                    return Err("property type mismatch".into());
                }
                let compare = if matches!(rule, Property::OnlyValue { .. }) {
                    "!="
                } else {
                    "="
                };
                Ok(format!(
                    "some r:{} | {live} and r.c{column} {compare} {}",
                    output.name,
                    self.atom(value)
                ))
            }
        }
    }
}

/// Emit one binary operation with explicit boolean and NULL-aware comparison functions.
fn binary(op: BinaryOp, a: &str, b: &str) -> String {
    match op {
        BinaryOp::Eq => format!("eqv[{a},{b}]"),
        BinaryOp::Ne => format!("notv[eqv[{a},{b}]]"),
        BinaryOp::Lt => format!("ltv[{a},{b}]"),
        BinaryOp::Gt => format!("ltv[{b},{a}]"),
        BinaryOp::Le => format!("notv[ltv[{b},{a}]]"),
        BinaryOp::Ge => format!("notv[ltv[{a},{b}]]"),
        BinaryOp::And => format!("andv[{a},{b}]"),
        BinaryOp::Or => format!("orv[{a},{b}]"),
    }
}

/// Refer to every ordered cell position of a relation occurrence.
pub(super) fn cells(row: &str, width: usize) -> Vec<String> {
    (0..width).map(|i| format!("{row}.c{i}")).collect()
}

/// Duplicate equivalence includes NULL=NULL, unlike predicate equality.
pub(super) fn row_equal(left: &str, right: &str, width: usize) -> String {
    let terms: Vec<_> = (0..width)
        .map(|i| format!("{left}.c{i}={right}.c{i}"))
        .collect();
    if terms.is_empty() {
        "no none".into()
    } else {
        terms.join(" and ")
    }
}
