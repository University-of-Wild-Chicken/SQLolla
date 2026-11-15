//! Recursive-descent regex parser with explicit node, nesting and repetition bounds.
use super::{syntax, Node, Program};
use crate::json::error;
use crate::Result;

struct Parser {
    input: Vec<char>,
    position: usize,
    nodes: Vec<Node>,
    depth: usize,
}

/// Parse a whole pattern; unsupported ARE features are never silently treated as literals.
pub(super) fn parse(pattern: &str) -> Result<Program> {
    let mut parser = Parser {
        input: pattern.chars().collect(),
        position: 0,
        nodes: Vec::new(),
        depth: 0,
    };
    let root = parser.alternation()?;
    if parser.position != parser.input.len() {
        return Err(syntax("Unmatched closing parenthesis"));
    }
    Ok(Program {
        nodes: parser.nodes,
        root,
    })
}

impl Parser {
    /// Look ahead without consuming a character.
    fn peek(&self) -> Option<char> {
        self.input.get(self.position).copied()
    }
    /// Consume one character if the pattern has not ended.
    fn next(&mut self) -> Option<char> {
        let value = self.peek();
        self.position += usize::from(value.is_some());
        value
    }
    /// Store a node only while the complete pattern fits the advertised syntax budget.
    fn node(&mut self, node: Node) -> Result<usize> {
        if self.nodes.len() >= 256 {
            return Err(error(
                "resource.regex_nodes",
                "Regex syntax exceeds node budget",
            ));
        }
        self.nodes.push(node);
        Ok(self.nodes.len() - 1)
    }
    /// Alternation branches remain generic syntax rather than a preselected corpus pattern.
    fn alternation(&mut self) -> Result<usize> {
        let mut branches = vec![self.sequence()?];
        while self.peek() == Some('|') {
            self.next();
            branches.push(self.sequence()?);
        }
        if branches.len() == 1 {
            Ok(branches[0])
        } else {
            self.node(Node::Alternate(branches))
        }
    }
    /// Empty branches are legitimate zero-width regular expressions.
    fn sequence(&mut self) -> Result<usize> {
        let mut values = Vec::new();
        while !matches!(self.peek(), None | Some('|' | ')')) {
            let atom = self.atom()?;
            values.push(self.quantified(atom)?);
        }
        match values.len() {
            0 => self.node(Node::Empty),
            1 => Ok(values[0]),
            _ => self.node(Node::Sequence(values)),
        }
    }
    /// Parse literals, anchors, groups and explicitly supported character classes.
    fn atom(&mut self) -> Result<usize> {
        let value = self.next().ok_or_else(|| syntax("Missing regex atom"))?;
        let node = match value {
            '(' => return self.group(),
            '[' => return self.class(),
            '\\' => Node::Literal(self.escape()?),
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '*' | '+' | '?' => return Err(syntax("Quantifier has no operand")),
            '{' => {
                return Err(error(
                    "unsupported.regex_braces",
                    "Unescaped brace literals need an explicit parsing profile",
                ));
            }
            value => Node::Literal(value),
        };
        self.node(node)
    }
    /// Groups are semantic grouping only; lookaround and embedded options are explicit refusals.
    fn group(&mut self) -> Result<usize> {
        if self.peek() == Some('?') {
            return Err(error(
                "unsupported.regex_group",
                "ARE group extensions require separate contracts",
            ));
        }
        self.depth += 1;
        if self.depth > 32 {
            return Err(error(
                "resource.regex_depth",
                "Regex group nesting exceeds bound",
            ));
        }
        let result = self.alternation()?;
        self.depth -= 1;
        if self.next() != Some(')') {
            return Err(syntax("Unclosed group"));
        }
        Ok(result)
    }
    /// Punctuation escapes are literal; locale classes/backreferences are never guessed.
    fn escape(&mut self) -> Result<char> {
        match self.next().ok_or_else(|| syntax("Trailing regex escape"))? {
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            value if value.is_ascii_alphanumeric() => Err(error(
                "unsupported.regex_escape",
                "ARE character escapes or backreferences require a separate profile",
            )),
            value => Ok(value),
        }
    }
    /// Character classes support exact literals and ASCII ranges under the C comparison profile.
    fn class(&mut self) -> Result<usize> {
        let negated = self.peek() == Some('^');
        if negated {
            self.next();
        }
        let mut ranges = Vec::new();
        while self.peek() != Some(']') || ranges.is_empty() {
            let first = self.class_character()?;
            let last =
                if self.peek() == Some('-') && self.input.get(self.position + 1) != Some(&']') {
                    self.next();
                    self.class_character()?
                } else {
                    first
                };
            if first != last && (!first.is_ascii() || !last.is_ascii()) {
                return Err(error(
                    "unsupported.regex_collation",
                    "Non-ASCII ranges require collation semantics",
                ));
            }
            if first > last {
                return Err(syntax("Invalid character range"));
            }
            ranges.push((first, last));
        }
        self.next();
        self.node(Node::Class { negated, ranges })
    }
    /// Distinguish POSIX classes/collating elements from ordinary bracket characters.
    fn class_character(&mut self) -> Result<char> {
        let value = self
            .next()
            .ok_or_else(|| syntax("Unclosed character class"))?;
        if value == '[' && matches!(self.peek(), Some(':' | '.' | '=')) {
            return Err(error(
                "unsupported.regex_class",
                "Locale-aware bracket classes are not admitted",
            ));
        }
        if value == '\\' {
            self.escape()
        } else {
            Ok(value)
        }
    }
    /// Repetition bounds are finite or an explicit closure; lazy syntax does not change Boolean membership.
    fn quantified(&mut self, child: usize) -> Result<usize> {
        if matches!(self.nodes[child], Node::Start | Node::End)
            && matches!(self.peek(), Some('*' | '+' | '?' | '{'))
        {
            return Err(error(
                "unsupported.regex_anchor_repeat",
                "Repeated constraints need separate syntax validation",
            ));
        }
        let Some((minimum, maximum)) = self.repeat_bounds()? else {
            return Ok(child);
        };
        if self.peek() == Some('?') {
            self.next();
        }
        if matches!(self.peek(), Some('*' | '+' | '?' | '{')) {
            return Err(syntax("Repeated quantifier"));
        }
        self.node(Node::Repeat {
            child,
            minimum,
            maximum,
        })
    }
    /// Decode one repeat marker without conflating a literal atom with its quantifier.
    fn repeat_bounds(&mut self) -> Result<Option<(usize, Option<usize>)>> {
        match self.peek() {
            Some('*' | '+' | '?') => Ok(Some(match self.next() {
                Some('*') => (0, None),
                Some('+') => (1, None),
                _ => (0, Some(1)),
            })),
            Some('{') => self.bounds().map(Some),
            _ => Ok(None),
        }
    }
    /// Bounded repeats are admitted through PostgreSQL's documented maximum of 255.
    fn bounds(&mut self) -> Result<(usize, Option<usize>)> {
        self.next();
        let minimum = self.number()?;
        let maximum = if self.peek() == Some(',') {
            self.next();
            if self.peek() == Some('}') {
                None
            } else {
                Some(self.number()?)
            }
        } else {
            Some(minimum)
        };
        if self.next() != Some('}') {
            return Err(error("unsupported.regex_braces", "Unsupported brace form"));
        }
        if maximum.is_some_and(|maximum| maximum < minimum) {
            return Err(syntax("Repeat bounds are reversed"));
        }
        Ok((minimum, maximum))
    }
    /// Parse a checked repeat count; oversized cases refuse rather than truncating the automaton.
    fn number(&mut self) -> Result<usize> {
        let mut value = 0usize;
        let start = self.position;
        while self.peek().is_some_and(|value| value.is_ascii_digit()) {
            value = value * 10 + self.next().unwrap_or('0').to_digit(10).unwrap_or(0) as usize;
            if value > 255 {
                return Err(error(
                    "resource.regex_repeat",
                    "Repeat count exceeds the admitted bound",
                ));
            }
        }
        if self.position == start {
            return Err(error(
                "unsupported.regex_braces",
                "Brace text is outside the bounded-repeat grammar",
            ));
        }
        Ok(value)
    }
}
