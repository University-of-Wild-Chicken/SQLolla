//! Canonical cell atoms, finite scopes and relation slot allocation.
use super::{Encoded, Inputs, Layout, MAX_SLOTS, Property};
use sqlolla_core::{BoundExpr, Cell, ExprNode, Plan, RelNode, SqlType};
use std::collections::{BTreeMap, BTreeSet};

/// Mutable emission state is private; public compilation is a pure transformation.
pub(super) struct Builder<'a> {
    pub inputs: Inputs<'a>,
    pub atoms: BTreeMap<Cell, String>,
    pub tables: BTreeMap<String, Layout>,
    pub nodes: Vec<Layout>,
    pub facts: Vec<String>,
    pub slots: usize,
    pub cells: usize,
}

impl<'a> Builder<'a> {
    /// Collect exact scalar values before assigning stable compiler-owned names.
    pub fn new(
        plan: &Plan,
        inputs: Inputs<'a>,
        property: Option<&Property>,
    ) -> Result<Self, String> {
        let mut values = BTreeSet::from([Cell::Bool(false), Cell::Bool(true)]);
        collect_plan(plan, &mut values);
        collect_inputs(&inputs, &mut values);
        if let Some(Property::NoValue { value, .. } | Property::OnlyValue { value, .. }) = property
        {
            values.insert(value.clone());
        }
        if values.len() > 128 {
            return Err("value domain exceeds 128 cells".into());
        }
        let atoms = canonical_atoms(values);
        Ok(Self {
            inputs,
            atoms,
            tables: BTreeMap::new(),
            nodes: Vec::new(),
            facts: Vec::new(),
            slots: 0,
            cells: 0,
        })
    }

    /// Build constraints and both reachability/property commands with explicit scopes.
    pub fn compile(mut self, plan: &Plan, property: Option<&Property>) -> Result<Encoded, String> {
        let output = self.relation(plan)?;
        let mut model = self.prelude();
        for node in &self.nodes {
            model.push_str(&declaration(node));
        }
        model.push_str(&format!(
            "pred QuerySemantics {{\n{}\n}}\n",
            self.facts.join("\n")
        ));
        let scope = self.scope();
        model.push_str(&format!("run QuerySemantics {scope}\n"));
        if let Some(rule) = property {
            let body = self.property(rule, &output)?;
            model.push_str(&super::property_check("QuerySemantics", &body, &scope));
        }
        let values = self.atoms.into_iter().map(|(v, k)| (k, v)).collect();
        if model.len() > 2_097_152 {
            return Err("generated model exceeds two MiB".into());
        }
        Ok(Encoded {
            model,
            output,
            inputs: self.tables,
            values,
        })
    }

    /// Allocate every candidate occurrence; reject excessive capacity before emitting a model.
    pub fn allocate(
        &mut self,
        schema: &[sqlolla_core::Column],
        count: usize,
    ) -> Result<Layout, String> {
        self.check_capacity(schema.len(), count)?;
        self.slots = self
            .slots
            .checked_add(count)
            .ok_or("slot capacity overflow")?;
        if self.slots > MAX_SLOTS {
            return Err("complete occurrence capacity exceeds 256 slots".into());
        }
        let name = format!("N{}", self.nodes.len());
        let rows = (0..count).map(|i| format!("{name}R{i}")).collect();
        let layout = Layout {
            name,
            rows,
            schema: schema.to_vec(),
        };
        self.nodes.push(layout.clone());
        Ok(layout)
    }

    /// Reject excessive width/node/cell budgets before materializing occurrence constraints.
    fn check_capacity(&mut self, width: usize, count: usize) -> Result<(), String> {
        if width > 256 || self.nodes.len() >= 256 {
            return Err("schema width or relation count exceeds 256".into());
        }
        let cells = width.checked_mul(count).ok_or("cell capacity overflow")?;
        self.cells = self
            .cells
            .checked_add(cells)
            .ok_or("cell capacity overflow")?;
        if self.cells > 16_384 {
            return Err("complete cell capacity exceeds 16384".into());
        }
        Ok(())
    }

    /// Resolve a typed NULL or an exact literal to its unambiguous Alloy atom.
    pub fn atom(&self, value: &Cell) -> String {
        match value {
            Cell::Null(_) => "Null".into(),
            _ => self.atoms[value].clone(),
        }
    }

    /// Assign the live flag and every column for one output occurrence.
    pub fn row(&mut self, target: &str, condition: &str, cells: &[String]) {
        let yes = self.atom(&Cell::Bool(true));
        let no = self.atom(&Cell::Bool(false));
        self.facts.push(format!(
            "{target}.alive = (({condition}) => {yes} else {no})"
        ));
        for (index, value) in cells.iter().enumerate() {
            self.facts.push(format!("{target}.c{index} = {value}"));
        }
    }

    /// Match a live occurrence without treating NULL as TRUE.
    pub fn live(&self, row: &str) -> String {
        format!("{row}.alive = {}", self.atom(&Cell::Bool(true)))
    }

    /// Provide enough signed integer range for every possible cardinality, not SQL arithmetic.
    fn scope(&self) -> String {
        let bitwidth = (usize::BITS - self.slots.max(1).leading_zeros() + 1).max(4);
        let mut scopes = vec![
            format!("{bitwidth} Int"),
            format!("exactly {} V", self.atoms.len() + 1),
        ];
        scopes.extend(
            self.nodes
                .iter()
                .map(|n| format!("exactly {} {}", n.rows.len(), n.name)),
        );
        format!("for 0 but {}", scopes.join(", "))
    }
}

/// Assign stable scalar identities independently of insertion and database row order.
fn canonical_atoms(values: BTreeSet<Cell>) -> BTreeMap<Cell, String> {
    values
        .into_iter()
        .filter(|v| !matches!(v, Cell::Null(_)))
        .enumerate()
        .map(|(i, v)| (v, format!("V{i}")))
        .collect()
}

/// Declare columns and singleton occurrence identities without value-based deduplication.
fn declaration(node: &Layout) -> String {
    let mut fields = vec!["alive: one V".to_owned()];
    fields.extend((0..node.schema.len()).map(|i| format!("c{i}: one V")));
    let mut result = format!("abstract sig {} {{ {} }}\n", node.name, fields.join(", "));
    for row in &node.rows {
        result.push_str(&format!("one sig {row} extends {} {{}}\n", node.name));
    }
    result
}

/// Include input values but do not infer an unbounded domain from them.
fn collect_inputs(inputs: &Inputs<'_>, values: &mut BTreeSet<Cell>) {
    match inputs {
        Inputs::Concrete(db) => values.extend(db.values().flatten().flatten().cloned()),
        Inputs::Symbolic { domain, .. } => values.extend(domain.iter().cloned()),
    }
}

/// Visit every literal in the typed plan without evaluating its operators.
fn collect_plan(plan: &Plan, values: &mut BTreeSet<Cell>) {
    match &plan.node {
        RelNode::Values { rows } => values.extend(rows.iter().flatten().cloned()),
        RelNode::Scan { .. } => (),
        RelNode::Filter { input, predicate } => {
            collect_plan(input, values);
            collect_expr(predicate, values);
        }
        RelNode::Project { input, expressions } => {
            collect_plan(input, values);
            for expression in expressions {
                collect_expr(expression, values);
            }
        }
        RelNode::Distinct { input } => collect_plan(input, values),
        RelNode::Product { left, right } | RelNode::Set { left, right, .. } => {
            collect_plan(left, values);
            collect_plan(right, values);
        }
        RelNode::Join {
            left, right, on, ..
        } => {
            collect_plan(left, values);
            collect_plan(right, values);
            collect_expr(on, values);
        }
        _ => (),
    }
}

/// Collect scalar literals recursively; column expressions add no new atoms.
fn collect_expr(expression: &BoundExpr, values: &mut BTreeSet<Cell>) {
    match &expression.node {
        ExprNode::Literal(cell) => {
            values.insert(cell.clone());
        }
        ExprNode::Column(_) => (),
        ExprNode::Not(arg) | ExprNode::IsNull { arg, .. } => collect_expr(arg, values),
        ExprNode::Binary { left, right, .. } => {
            collect_expr(left, values);
            collect_expr(right, values);
        }
        #[allow(unreachable_patterns)]
        _ => (),
    }
}

/// Return the declared scalar family of an input value.
pub(super) fn cell_type(cell: &Cell) -> SqlType {
    cell.sql_type()
}
