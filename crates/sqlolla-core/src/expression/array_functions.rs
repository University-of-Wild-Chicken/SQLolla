//! Polymorphic array signatures and slice syntax lower to explicit typed scalar operations.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Bind array builtins with PostgreSQL-compatible array/element common-type promotion.
    pub(super) fn array_call(&self, node: &Value, name: &str) -> Result<Option<BoundExpr>> {
        if !matches!(
            name,
            "array_append" | "array_prepend" | "array_length" | "cardinality"
        ) {
            return Ok(None);
        }
        let nodes = array(node, "args")?;
        let args = if matches!(name, "array_append" | "array_prepend") {
            self.array_push_args(nodes, name == "array_prepend")?
        } else {
            self.array_measure_args(nodes, name == "cardinality")?
        };
        let ty = scalar::return_type(
            name,
            &args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>(),
        )?;
        Ok(Some(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Call {
                name: name.into(),
                args,
            },
        }))
    }

    /// Resolve both array and appended element before binding unknown literals.
    fn array_push_args(&self, nodes: &[Value], prepend: bool) -> Result<Vec<BoundExpr>> {
        if nodes.len() != 2 {
            return Err(error("sql.42883", "Array append/prepend arity"));
        }
        let (base, value) = if prepend {
            (&nodes[1], &nodes[0])
        } else {
            (&nodes[0], &nodes[1])
        };
        let element = match self.hint(base)? {
            Some(SqlType::Array(element)) => Some(*element),
            Some(_) => return Err(error("sql.42883", "Expected an array argument")),
            None => None,
        };
        let element = match (element, self.hint(value)?) {
            (Some(left), Some(right)) => scalar::common_type(&left, &right)
                .map_err(|_| error("sql.42883", "Array/element families do not match"))?,
            (Some(ty), None) | (None, Some(ty)) => ty,
            (None, None) => SqlType::Text,
        };
        let base = self.bind(base, Some(SqlType::Array(Box::new(element.clone()))))?;
        let value = self.bind(value, Some(element))?;
        Ok(if prepend {
            vec![value, base]
        } else {
            vec![base, value]
        })
    }

    /// Array measurements need a resolved array family and an implicitly integral dimension.
    fn array_measure_args(&self, nodes: &[Value], cardinality: bool) -> Result<Vec<BoundExpr>> {
        if nodes.len() != if cardinality { 1 } else { 2 } {
            return Err(error("sql.42883", "Array measurement arity"));
        }
        let Some(ty @ SqlType::Array(_)) = self.hint(&nodes[0])? else {
            return Err(error(
                "sql.42804",
                "Cannot resolve polymorphic array argument",
            ));
        };
        let mut args = vec![self.bind(&nodes[0], Some(ty))?];
        if !cardinality {
            args.push(self.collection_argument(&nodes[1], SqlType::Int32)?);
        }
        Ok(args)
    }

    /// A colon in any dimension changes every selector into a slice selector.
    pub(super) fn array_slice(&self, base: BoundExpr, indices: &[Value]) -> Result<BoundExpr> {
        let ty = base.ty.clone();
        let mut args = vec![base];
        for index in indices {
            if text(index, "tag")? == "index" {
                args.extend([
                    literal(Cell::Bool(true)),
                    literal(Cell::Int32(1)),
                    literal(Cell::Bool(true)),
                    self.array_index(&index["expression"])?,
                ]);
            } else {
                fields(index, &["tag", "lower", "upper"])?;
                args.extend(self.slice_endpoint(&index["lower"])?);
                args.extend(self.slice_endpoint(&index["upper"])?);
            }
        }
        Ok(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Call {
                name: "__sqlolla_array_slice".into(),
                args,
            },
        })
    }

    /// Omitted bounds carry a false presence bit; explicit SQL NULL remains an expression.
    fn slice_endpoint(&self, node: &Value) -> Result<[BoundExpr; 2]> {
        if node.is_null() {
            return Ok([literal(Cell::Bool(false)), literal(Cell::Int32(0))]);
        }
        Ok([literal(Cell::Bool(true)), self.array_index(node)?])
    }
}
