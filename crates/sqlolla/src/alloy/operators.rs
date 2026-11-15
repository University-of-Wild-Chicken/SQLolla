//! Relational constraints retain a complete occurrence slot for every candidate row.
use super::builder::{Builder, cell_type};
use super::expressions::cells;
use super::{Inputs, Layout};
use sqlolla_core::{Bag, BoundExpr, Cell, JoinKind, Plan, RelNode};

impl Builder<'_> {
    /// Lower each admitted relational constructor to deterministic constraints.
    pub fn relation(&mut self, plan: &Plan) -> Result<Layout, String> {
        match &plan.node {
            RelNode::Values { rows } => self.values(plan, rows),
            RelNode::Scan { table } => self.scan(plan, table),
            RelNode::Filter { input, predicate } => self.unary(plan, input, Some(predicate), None),
            RelNode::Project { input, expressions } => {
                self.unary(plan, input, None, Some(expressions))
            }
            RelNode::Product { left, right } => self.join(plan, left, right, None, JoinKind::Inner),
            RelNode::Join {
                kind,
                left,
                right,
                on,
            } => self.join(plan, left, right, Some(on), *kind),
            RelNode::Distinct { input } => self.distinct(plan, input),
            RelNode::Set {
                op,
                all,
                left,
                right,
            } => self.set(plan, left, right, *op, *all),
            _ => Err("operator requires the explicit outcome encoding profile".into()),
        }
    }

    /// VALUES occurrences stay separate even when their cells are identical.
    fn values(&mut self, plan: &Plan, rows: &Bag) -> Result<Layout, String> {
        let layout = self.allocate(&plan.schema, rows.len())?;
        for (target, values) in layout.rows.iter().zip(rows) {
            let values = values.iter().map(|v| self.atom(v)).collect::<Vec<_>>();
            self.row(target, "no none", &values);
        }
        Ok(layout)
    }

    /// Reuse one base relation for self-joins so aliases cannot invent separate databases.
    fn scan(&mut self, plan: &Plan, table: &str) -> Result<Layout, String> {
        if let Some(layout) = self.tables.get(table) {
            return Ok(layout.clone());
        }
        let layout = match &self.inputs {
            Inputs::Concrete(database) => {
                let rows = database.get(table).ok_or("missing database table")?.clone();
                self.values(plan, &rows)?
            }
            Inputs::Symbolic { bounds, domain } => {
                let count = *bounds.get(table).ok_or("missing table row bound")?;
                let domain = domain.to_vec();
                self.symbolic(plan, count, &domain)?
            }
        };
        self.tables.insert(table.into(), layout.clone());
        Ok(layout)
    }

    /// Give every symbolic slot optional presence and a closed, typed cell domain.
    fn symbolic(&mut self, plan: &Plan, count: usize, domain: &[Cell]) -> Result<Layout, String> {
        let layout = self.allocate(&plan.schema, count)?;
        let t = self.atom(&Cell::Bool(true));
        let f = self.atom(&Cell::Bool(false));
        self.facts
            .push(format!("all r:{} | r.alive in {t}+{f}", layout.name));
        for (i, column) in layout.schema.iter().enumerate() {
            let allowed = self.column_domain(column, domain, count)?;
            self.facts.push(format!(
                "all r:{} | r.alive={t} implies r.c{i} in ({allowed})",
                layout.name
            ));
            self.facts.push(format!(
                "all r:{} | r.alive={f} implies r.c{i}=Null",
                layout.name
            ));
        }
        Ok(layout)
    }

    /// Admit exactly the declared finite values of one nullable SQL column.
    fn column_domain(
        &self,
        column: &sqlolla_core::Column,
        domain: &[Cell],
        count: usize,
    ) -> Result<String, String> {
        let values: Vec<_> = domain
            .iter()
            .filter(|v| cell_type(v) == column.ty)
            .filter(|v| column.nullable || !matches!(v, Cell::Null(_)))
            .map(|v| self.atom(v))
            .collect();
        if values.is_empty() && count > 0 {
            return Err("symbolic domain omits a column type".into());
        }
        Ok(if values.is_empty() {
            "none".into()
        } else {
            values.join("+")
        })
    }

    /// Filtering controls presence; projection computes every output without deduplicating.
    fn unary(
        &mut self,
        plan: &Plan,
        input: &Plan,
        predicate: Option<&BoundExpr>,
        projection: Option<&Vec<BoundExpr>>,
    ) -> Result<Layout, String> {
        let source = self.relation(input)?;
        let target = self.allocate(&plan.schema, source.rows.len())?;
        for (from, to) in source.rows.iter().zip(&target.rows) {
            let mut values = cells(from, source.schema.len());
            let mut condition = self.live(from);
            if let Some(p) = predicate {
                condition.push_str(&format!(
                    " and {}={}",
                    self.expression(p, &values)?,
                    self.atom(&Cell::Bool(true))
                ));
            }
            if let Some(expressions) = projection {
                values = expressions
                    .iter()
                    .map(|e| self.expression(e, &values))
                    .collect::<Result<_, _>>()?;
            }
            self.row(to, &condition, &values);
        }
        Ok(target)
    }

    /// Allocate all pair slots and all required null-extension slots before applying ON.
    fn join(
        &mut self,
        plan: &Plan,
        left: &Plan,
        right: &Plan,
        on: Option<&BoundExpr>,
        kind: JoinKind,
    ) -> Result<Layout, String> {
        let l = self.relation(left)?;
        let r = self.relation(right)?;
        let (pairs, extra_l, extra_r) = join_capacity(&l, &r, kind)?;
        let target = self.allocate(&plan.schema, pairs + extra_l + extra_r)?;
        self.join_pairs(&l, &r, on, &target)?;
        if extra_l > 0 {
            self.unmatched(&l, &r, on, &target.rows[pairs..pairs + extra_l], false)?;
        }
        if extra_r > 0 {
            self.unmatched(&r, &l, on, &target.rows[pairs + extra_l..], true)?;
        }
        Ok(target)
    }

    /// Encode every candidate pair with its own live predicate and column sequence.
    fn join_pairs(
        &mut self,
        left: &Layout,
        right: &Layout,
        on: Option<&BoundExpr>,
        target: &Layout,
    ) -> Result<(), String> {
        let mut index = 0;
        for a in &left.rows {
            for b in &right.rows {
                let values = [cells(a, left.schema.len()), cells(b, right.schema.len())].concat();
                let condition = self.matching(a, b, &values, on)?;
                self.row(&target.rows[index], &condition, &values);
                index += 1;
            }
        }
        Ok(())
    }

    /// Determine ON matching with TRUE-only SQL semantics.
    fn matching(
        &self,
        left: &str,
        right: &str,
        values: &[String],
        on: Option<&BoundExpr>,
    ) -> Result<String, String> {
        let mut condition = format!("{} and {}", self.live(left), self.live(right));
        if let Some(on) = on {
            condition.push_str(&format!(
                " and {}={}",
                self.expression(on, values)?,
                self.atom(&Cell::Bool(true))
            ));
        }
        Ok(condition)
    }

    /// Outer rows appear exactly when no live counterpart makes ON TRUE.
    fn unmatched(
        &mut self,
        side: &Layout,
        other: &Layout,
        on: Option<&BoundExpr>,
        targets: &[String],
        reverse: bool,
    ) -> Result<(), String> {
        for (row, target) in side.rows.iter().zip(targets) {
            let own = cells(row, side.schema.len());
            let peer = cells("p", other.schema.len());
            let pair = if reverse {
                [peer, own.clone()].concat()
            } else {
                [own.clone(), peer].concat()
            };
            let matched = self.matching(row, "p", &pair, on)?;
            let condition = format!("{} and no p:{} | {matched}", self.live(row), other.name);
            let nulls = vec!["Null".into(); other.schema.len()];
            let values = if reverse {
                [nulls, own].concat()
            } else {
                [own, nulls].concat()
            };
            self.row(target, &condition, &values);
        }
        Ok(())
    }
}

/// Reserve matched pairs and optional unmatched sides before producing any constraints.
fn join_capacity(
    left: &Layout,
    right: &Layout,
    kind: JoinKind,
) -> Result<(usize, usize, usize), String> {
    let pairs = left
        .rows
        .len()
        .checked_mul(right.rows.len())
        .ok_or("join capacity overflow")?;
    let l = if matches!(kind, JoinKind::Left | JoinKind::Full) {
        left.rows.len()
    } else {
        0
    };
    let r = if matches!(kind, JoinKind::Right | JoinKind::Full) {
        right.rows.len()
    } else {
        0
    };
    Ok((pairs, l, r))
}
