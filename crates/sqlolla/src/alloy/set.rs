//! DISTINCT and set operators use duplicate equivalence and occurrence rank.
use super::Layout;
use super::builder::Builder;
use super::expressions::{cells, row_equal};
use sqlolla_core::{Plan, SetOp};

impl Builder<'_> {
    /// DISTINCT retains one canonical representative without imposing result ordering.
    pub fn distinct(&mut self, plan: &Plan, input: &Plan) -> Result<Layout, String> {
        let source = self.relation(input)?;
        self.deduplicate(plan, &source)
    }

    /// Preserve the first live occurrence of each duplicate-equivalent value tuple.
    fn deduplicate(&mut self, plan: &Plan, source: &Layout) -> Result<Layout, String> {
        let target = self.allocate(&plan.schema, source.rows.len())?;
        for (index, row) in source.rows.iter().enumerate() {
            let prior = self.matches(source, row, Some(index));
            let condition = format!("{} and no ({prior})", self.live(row));
            self.row(
                &target.rows[index],
                &condition,
                &cells(row, source.schema.len()),
            );
        }
        Ok(target)
    }

    /// Implement each ALL flag explicitly instead of replacing bag operations with sets.
    pub fn set(
        &mut self,
        plan: &Plan,
        left: &Plan,
        right: &Plan,
        op: SetOp,
        all: bool,
    ) -> Result<Layout, String> {
        let l = self.relation(left)?;
        let r = self.relation(right)?;
        let target = match op {
            SetOp::Union => self.union(plan, &l, &r)?,
            SetOp::Intersect | SetOp::Except => self.ranked(plan, &l, &r, op, all)?,
        };
        if !all {
            self.deduplicate(plan, &target)
        } else {
            Ok(target)
        }
    }

    /// UNION ALL allocates disjoint output occurrences for both branches.
    fn union(&mut self, plan: &Plan, left: &Layout, right: &Layout) -> Result<Layout, String> {
        let target = self.allocate(&plan.schema, left.rows.len() + right.rows.len())?;
        for (from, to) in left.rows.iter().chain(&right.rows).zip(&target.rows) {
            self.row(to, &self.live(from), &cells(from, target.schema.len()));
        }
        Ok(target)
    }

    /// Compare matching occurrence counts for ALL, or existence for DISTINCT variants.
    fn ranked(
        &mut self,
        plan: &Plan,
        left: &Layout,
        right: &Layout,
        op: SetOp,
        all: bool,
    ) -> Result<Layout, String> {
        let target = self.allocate(&plan.schema, left.rows.len())?;
        for (index, row) in left.rows.iter().enumerate() {
            let prior = self.matches(left, row, Some(index));
            let peers = self.matches(right, row, None);
            let condition = match (op, all) {
                (SetOp::Intersect, true) => format!("#({prior}) < #({peers})"),
                (SetOp::Except, true) => format!("#({prior}) >= #({peers})"),
                (SetOp::Intersect, false) => format!("some ({peers})"),
                _ => format!("no ({peers})"),
            };
            self.row(
                &target.rows[index],
                &format!("{} and ({condition})", self.live(row)),
                &cells(row, target.schema.len()),
            );
        }
        Ok(target)
    }

    /// Select equal-valued live slots, optionally restricted to a known earlier prefix.
    fn matches(&self, source: &Layout, row: &str, prefix: Option<usize>) -> String {
        let allowed = prefix.map(|n| source.rows[..n].join("+"));
        let restriction = match allowed {
            Some(names) if names.is_empty() => "p in none and ".into(),
            Some(names) => format!("p in ({names}) and "),
            None => String::new(),
        };
        format!(
            "{{p:{} | {restriction}{} and {}}}",
            source.name,
            self.live("p"),
            row_equal("p", row, source.schema.len())
        )
    }
}
