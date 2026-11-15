//! Bounded positional matcher for an explicit regular-language subset of PostgreSQL ARE.
mod matcher;
mod parser;
use crate::json::error;
use crate::{Cell, Result, SqlType};

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Literal(char),
    Any,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    Start,
    End,
    Sequence(Vec<usize>),
    Alternate(Vec<usize>),
    Repeat {
        child: usize,
        minimum: usize,
        maximum: Option<usize>,
    },
}

#[derive(Debug)]
struct Program {
    nodes: Vec<Node>,
    root: usize,
}

/// Match the admitted PostgreSQL regex language; exhaustion is an error, never false.
pub fn regex_match(text: &str, pattern: &str, insensitive: bool) -> Result<bool> {
    if pattern.len() > 2048 || text.chars().count() > 4096 {
        return Err(error(
            "resource.regex_size",
            "Regex pattern/input exceeds its explicit bound",
        ));
    }
    if insensitive && (!text.is_ascii() || !pattern.is_ascii()) {
        return Err(error(
            "unsupported.regex_case_locale",
            "Case-insensitive Unicode regex requires its locale profile",
        ));
    }
    let program = parser::parse(pattern)?;
    matcher::search(&program, text, insensitive)
}

/// Internal operator execution retains SQL strict NULL and independent Boolean negation.
pub(crate) fn evaluate(args: &[Cell]) -> Result<Cell> {
    validate_arguments(args)?;
    if args[..2].iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Bool));
    }
    let (Cell::Text(text), Cell::Text(pattern), Cell::Bool(insensitive), Cell::Bool(negated)) =
        (&args[0], &args[1], &args[2], &args[3])
    else {
        return Err(error(
            "invalid.regex_arguments",
            "Regex arguments are not resolved",
        ));
    };
    Ok(Cell::Bool(
        regex_match(text, pattern, *insensitive)? ^ *negated,
    ))
}

/// Internal callers still validate signature before the strict NULL shortcut.
fn validate_arguments(args: &[Cell]) -> Result<()> {
    if args.len() != 4 {
        return Err(error(
            "invalid.regex_arguments",
            "Expected text, pattern and two flags",
        ));
    }
    if args[0].sql_type() != SqlType::Text
        || args[1].sql_type() != SqlType::Text
        || !matches!((&args[2], &args[3]), (Cell::Bool(_), Cell::Bool(_)))
    {
        return Err(error(
            "invalid.regex_arguments",
            "Regex argument types are not resolved",
        ));
    }
    Ok(())
}

/// Invalid admitted syntax has PostgreSQL's invalid-regular-expression SQLSTATE.
fn syntax(message: &str) -> crate::Error {
    error("sql.2201B", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nullable repeated groups terminate while preserving every admitted suffix position.
    #[test]
    fn nullable_repeat_and_alternation() {
        assert!(regex_match("aaab", "^(a?)*b$", false).unwrap());
        assert!(!regex_match("aaac", "^(a?)*b$", false).unwrap());
        assert!(regex_match("ab", "^(a|ab)$", false).unwrap());
        assert!(regex_match("", "^(){2,}$", false).unwrap());
    }

    /// ASCII case folding does not erase punctuation inside an original range.
    #[test]
    fn ranges_and_unicode_literals() {
        assert!(regex_match("_", "^[A-z]$", true).unwrap());
        assert!(regex_match("é😀", "^é.$", false).unwrap());
        assert_eq!(
            regex_match("é", "É", true).unwrap_err().code,
            "unsupported.regex_case_locale"
        );
    }

    /// Exhaustion and unknown language constructs cannot be reported as negative matches.
    #[test]
    fn bounded_refusals() {
        assert_eq!(
            regex_match("a", "a{256}", false).unwrap_err().code,
            "resource.regex_repeat"
        );
        assert_eq!(
            regex_match("a", "(?=a)", false).unwrap_err().code,
            "unsupported.regex_group"
        );
        assert_eq!(
            regex_match("a", "[z-a]", false).unwrap_err().code,
            "sql.2201B"
        );
    }
}
