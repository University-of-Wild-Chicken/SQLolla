//! Memoized positional evaluation enumerates all endpoints within a strict local work budget.
use super::{Node, Program};
use crate::json::error;
use crate::Result;
use std::collections::{BTreeMap, BTreeSet};

struct Matcher<'a> {
    program: &'a Program,
    input: Vec<char>,
    insensitive: bool,
    memo: BTreeMap<(usize, usize), BTreeSet<usize>>,
    remaining: usize,
}

/// A Boolean regex search may start at every boundary, including the empty final suffix.
pub(super) fn search(program: &Program, text: &str, insensitive: bool) -> Result<bool> {
    let mut matcher = Matcher {
        program,
        input: text.chars().collect(),
        insensitive,
        memo: BTreeMap::new(),
        remaining: 100_000,
    };
    for start in 0..=matcher.input.len() {
        if !matcher.endpoints(program.root, start)?.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Matcher<'_> {
    /// Every expansion charges fuel; memoization never converts exhaustion into a negative match.
    fn charge(&mut self) -> Result<()> {
        self.remaining = self.remaining.checked_sub(1).ok_or_else(|| {
            error(
                "resource.regex_work",
                "Complete regex matching exceeds work budget",
            )
        })?;
        Ok(())
    }
    /// Cache the complete endpoint set for a node and start position.
    fn endpoints(&mut self, node: usize, start: usize) -> Result<BTreeSet<usize>> {
        self.charge()?;
        if let Some(values) = self.memo.get(&(node, start)) {
            return Ok(values.clone());
        }
        let values = self.evaluate(self.program.nodes[node].clone(), start)?;
        self.remaining = self.remaining.checked_sub(values.len()).ok_or_else(|| {
            error(
                "resource.regex_states",
                "Complete regex endpoint sets exceed capacity",
            )
        })?;
        self.memo.insert((node, start), values.clone());
        Ok(values)
    }
    /// Interpret structural regex nodes compositionally over input positions.
    fn evaluate(&mut self, node: Node, start: usize) -> Result<BTreeSet<usize>> {
        Ok(match node {
            Node::Empty => BTreeSet::from([start]),
            Node::Start if start == 0 => BTreeSet::from([start]),
            Node::End if start == self.input.len() => BTreeSet::from([start]),
            Node::Start | Node::End => BTreeSet::new(),
            Node::Sequence(nodes) => {
                let mut values = BTreeSet::from([start]);
                for node in nodes {
                    values = self.advance(node, &values)?;
                }
                values
            }
            Node::Alternate(nodes) => {
                let mut values = BTreeSet::new();
                for node in nodes {
                    values.extend(self.endpoints(node, start)?);
                }
                values
            }
            Node::Repeat {
                child,
                minimum,
                maximum,
            } => self.repeat(child, minimum, maximum, start)?,
            atom => self.atom_endpoints(&atom, start),
        })
    }
    /// A consuming atom advances exactly one Unicode scalar when its predicate accepts.
    fn atom_endpoints(&self, atom: &Node, start: usize) -> BTreeSet<usize> {
        if self
            .input
            .get(start)
            .is_some_and(|value| self.accepts(atom, *value))
        {
            BTreeSet::from([start + 1])
        } else {
            BTreeSet::new()
        }
    }
    /// Union complete child endpoints from every current position.
    fn advance(&mut self, node: usize, starts: &BTreeSet<usize>) -> Result<BTreeSet<usize>> {
        let mut values = BTreeSet::new();
        for start in starts {
            values.extend(self.endpoints(node, *start)?);
        }
        Ok(values)
    }
    /// Bounded repetition counts all repetitions; unbounded closure stops only at a fixed point.
    fn repeat(
        &mut self,
        child: usize,
        minimum: usize,
        maximum: Option<usize>,
        start: usize,
    ) -> Result<BTreeSet<usize>> {
        let mut frontier = BTreeSet::from([start]);
        for _ in 0..minimum {
            frontier = self.advance(child, &frontier)?;
        }
        let mut result = frontier.clone();
        let rounds = maximum
            .map(|maximum| maximum - minimum)
            .unwrap_or(self.input.len() + 1);
        for _ in 0..rounds {
            let next = self.advance(child, &frontier)?;
            if maximum.is_none() {
                frontier = next.difference(&result).copied().collect();
            } else {
                frontier = next;
            }
            if frontier.is_empty() {
                break;
            }
            result.extend(&frontier);
        }
        Ok(result)
    }
    /// Exact literals and ASCII ranges honor the separately admitted ASCII case-folding mode.
    fn accepts(&self, node: &Node, value: char) -> bool {
        match node {
            Node::Any => true,
            Node::Literal(expected) => self.fold(value) == self.fold(*expected),
            Node::Class { negated, ranges } => {
                ranges.iter().any(|range| self.in_range(value, *range)) != *negated
            }
            _ => false,
        }
    }
    /// Case-insensitive ranges retain punctuation already included by their original ASCII endpoints.
    fn in_range(&self, value: char, (first, last): (char, char)) -> bool {
        let contains = |value| first <= value && value <= last;
        contains(value)
            || (self.insensitive
                && (contains(value.to_ascii_lowercase()) || contains(value.to_ascii_uppercase())))
    }
    /// Unicode locale folding is refused before matcher construction.
    fn fold(&self, value: char) -> char {
        if self.insensitive {
            value.to_ascii_lowercase()
        } else {
            value
        }
    }
}
