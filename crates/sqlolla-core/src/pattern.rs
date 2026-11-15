//! Bounded SQL LIKE token matching, independent of regex syntax and engine defaults.
use crate::json::error;
use crate::*;

/// SQL wildcard tokens distinguish literal escaped characters from pattern operators.
enum Token {
    Literal(char),
    One,
    Many,
}

/// Apply strict NULL behavior before validating or matching a non-NULL pattern.
pub(crate) fn evaluate(
    arg: &BoundExpr,
    pattern: &BoundExpr,
    insensitive: bool,
    row: &Row,
) -> Result<Cell> {
    let arg = eval_expr(arg, row)?;
    let pattern = eval_expr(pattern, row)?;
    if arg.is_null() || pattern.is_null() {
        return Ok(Cell::Null(SqlType::Bool));
    }
    let (mut text, mut pattern) = (scalar::output(&arg)?, scalar::output(&pattern)?);
    normalize_case(&mut text, &mut pattern, insensitive)?;
    let tokens = tokens(&pattern)?;
    let text = text.chars().collect::<Vec<_>>();
    let work = text
        .len()
        .saturating_add(1)
        .saturating_mul(tokens.len().saturating_add(1));
    if work > 1_000_000 {
        return Err(error(
            "resource.pattern_work",
            "Complete LIKE matching exceeds work budget",
        ));
    }
    Ok(Cell::Bool(matches(&text, &tokens)))
}

/// The initial case-insensitive profile is ASCII under C collation, with explicit refusal beyond it.
fn normalize_case(text: &mut str, pattern: &mut str, insensitive: bool) -> Result<()> {
    if insensitive {
        if !text.is_ascii() || !pattern.is_ascii() {
            return Err(error(
                "unsupported.case_locale",
                "Non-ASCII ILIKE requires locale profile",
            ));
        }
        text.make_ascii_lowercase();
        pattern.make_ascii_lowercase();
    }
    Ok(())
}

/// A trailing default escape is SQLSTATE 22025; escaped wildcards remain literal characters.
fn tokens(pattern: &str) -> Result<Vec<Token>> {
    let mut result = Vec::new();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        result.push(match ch {
            '%' => Token::Many,
            '_' => Token::One,
            '\\' => Token::Literal(
                chars
                    .next()
                    .ok_or_else(|| error("sql.22025", "LIKE pattern ends with escape"))?,
            ),
            value => Token::Literal(value),
        });
    }
    Ok(result)
}

/// Dynamic programming enumerates every wildcard consumption within the declared work budget.
fn matches(text: &[char], tokens: &[Token]) -> bool {
    let mut previous = vec![false; text.len() + 1];
    previous[0] = true;
    for token in tokens {
        let mut next = vec![false; text.len() + 1];
        next[0] = matches!(token, Token::Many) && previous[0];
        for index in 1..=text.len() {
            next[index] = match token {
                Token::Many => previous[index] || next[index - 1],
                Token::One => previous[index - 1],
                Token::Literal(value) => previous[index - 1] && *value == text[index - 1],
            };
        }
        previous = next;
    }
    previous[text.len()]
}
