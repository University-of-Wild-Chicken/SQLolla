//! Rectangular PostgreSQL arrays retain dimensions, lower bounds and typed NULLs.
use crate::json::error;
use crate::{Cell, Result, SqlType};
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub(crate) struct ArrayValue {
    pub element: SqlType,
    pub dimensions: Vec<usize>,
    pub lower: Vec<i32>,
    pub values: Vec<Cell>,
}

impl ArrayValue {
    /// Parse rectangular array text with optional explicit dimension bounds.
    pub fn parse(element: &SqlType, value: &str) -> Result<Self> {
        let (body, lower, lengths) = dimensions(value.trim())?;
        let mut parser = Parser {
            characters: body.chars().collect(),
            position: 0,
            element,
        };
        let (dimensions, values) = parser.array(0)?;
        parser.space();
        if parser.position != parser.characters.len()
            || (!lengths.is_empty() && lengths != dimensions)
        {
            return invalid();
        }
        let lower = if lower.is_empty() {
            vec![1; dimensions.len()]
        } else {
            lower
        };
        validate_bounds(&lower, &dimensions)?;
        Ok(Self {
            element: element.clone(),
            dimensions,
            lower,
            values,
        })
    }
    /// Render array dimensions and quote every non-NULL scalar element explicitly.
    pub fn text(&self) -> Result<String> {
        let bounds = if self.lower.iter().all(|lower| *lower == 1) {
            String::new()
        } else {
            self.lower
                .iter()
                .zip(&self.dimensions)
                .map(|(lower, length)| {
                    format!("[{lower}:{}]", i64::from(*lower) + *length as i64 - 1)
                })
                .collect::<String>()
                + "="
        };
        let mut position = 0;
        Ok(bounds + &render(&self.dimensions, &self.values, &mut position)?)
    }
}

/// PostgreSQL also requires its exclusive upper boundary to fit a signed array index.
fn validate_bounds(lower: &[i32], dimensions: &[usize]) -> Result<()> {
    if lower
        .iter()
        .zip(dimensions)
        .any(|(lower, width)| i64::from(*lower) + *width as i64 > i64::from(i32::MAX))
    {
        return Err(error("sql.54000", "Array lower bound is too large"));
    }
    Ok(())
}

struct Parser<'a> {
    characters: Vec<char>,
    position: usize,
    element: &'a SqlType,
}
impl Parser<'_> {
    /// Skip array syntax whitespace without altering quoted element contents.
    fn space(&mut self) {
        while self
            .characters
            .get(self.position)
            .is_some_and(|c| c.is_ascii_whitespace())
        {
            self.position += 1;
        }
    }
    /// Parse one dimension and require a rectangular set of child dimensions.
    fn array(&mut self, depth: usize) -> Result<(Vec<usize>, Vec<Cell>)> {
        self.space();
        if depth >= 6 || self.characters.get(self.position) != Some(&'{') {
            return invalid();
        }
        self.position += 1;
        self.space();
        if self.characters.get(self.position) == Some(&'}') {
            self.position += 1;
            return Ok((vec![], vec![]));
        }
        self.contents(depth)
    }
    /// Collect one rectangular dimension after its opening delimiter.
    fn contents(&mut self, depth: usize) -> Result<(Vec<usize>, Vec<Cell>)> {
        let nested = self.characters.get(self.position) == Some(&'{');
        let mut values = Vec::new();
        let mut shape = None;
        let mut length = 0;
        loop {
            self.child(depth, nested, &mut shape, &mut values)?;
            length += 1;
            if self.separator()? {
                break;
            }
            if values.len() > 65536 {
                return Err(error("resource.array", "Array element budget exceeded"));
            }
        }
        let mut dimensions = vec![length];
        dimensions.extend(shape.unwrap_or_default());
        Ok((dimensions, values))
    }
    /// Require consistent child dimensions and append their row-major values.
    fn child(
        &mut self,
        depth: usize,
        nested: bool,
        shape: &mut Option<Vec<usize>>,
        values: &mut Vec<Cell>,
    ) -> Result<()> {
        self.space();
        if nested != (self.characters.get(self.position) == Some(&'{')) {
            return invalid();
        }
        if nested {
            let (dimensions, children) = self.array(depth + 1)?;
            if shape.as_ref().is_some_and(|shape| *shape != dimensions) || dimensions.is_empty() {
                return invalid();
            }
            *shape = Some(dimensions);
            values.extend(children);
        } else {
            values.push(self.element()?);
        }
        Ok(())
    }
    /// Consume a comma or closing brace and report whether this dimension ended.
    fn separator(&mut self) -> Result<bool> {
        self.space();
        let finished = match self.characters.get(self.position) {
            Some(',') => false,
            Some('}') => true,
            _ => return invalid(),
        };
        self.position += 1;
        Ok(finished)
    }
    /// Distinguish unquoted NULL from quoted text and decode backslash escapes.
    fn element(&mut self) -> Result<Cell> {
        self.space();
        let quoted = self.characters.get(self.position) == Some(&'"');
        if quoted {
            self.position += 1;
        }
        let value = self.token(quoted)?;
        let value = if quoted { value.as_str() } else { value.trim() };
        if !quoted && value.eq_ignore_ascii_case("NULL") {
            return Ok(Cell::Null(self.element.clone()));
        }
        if !quoted && value.is_empty() {
            return invalid();
        }
        super::input(self.element, value)
    }
    /// Decode one quoted or unquoted token while preserving escaped characters.
    fn token(&mut self, quoted: bool) -> Result<String> {
        let mut value = String::new();
        let mut closed = !quoted;
        while let Some(character) = self.characters.get(self.position).copied() {
            if quoted && character == '"' {
                self.position += 1;
                closed = true;
                break;
            }
            if !quoted && matches!(character, ',' | '}') {
                break;
            }
            self.position += 1;
            if character == '\\' {
                value.push(self.escaped()?);
            } else {
                value.push(character);
            }
        }
        if !closed {
            return invalid();
        }
        Ok(value)
    }
    /// Consume exactly one escaped character or reject an unfinished escape.
    fn escaped(&mut self) -> Result<char> {
        let character = *self
            .characters
            .get(self.position)
            .ok_or_else(|| error("sql.22P02", "Unclosed array escape"))?;
        self.position += 1;
        Ok(character)
    }
}

/// Parse explicit lower/upper bound pairs and leave the curly-braced value intact.
fn dimensions(mut value: &str) -> Result<(&str, Vec<i32>, Vec<usize>)> {
    let mut lower = Vec::new();
    let mut lengths = Vec::new();
    while value.starts_with('[') {
        let end = value
            .find(']')
            .ok_or_else(|| error("sql.22P02", "Unclosed array dimension"))?;
        let (start, stop) = value[1..end]
            .split_once(':')
            .ok_or_else(|| error("sql.22P02", "Invalid array dimension"))?;
        let start: i32 = start
            .parse()
            .map_err(|_| error("sql.22P02", "Invalid array lower bound"))?;
        let stop: i32 = stop
            .parse()
            .map_err(|_| error("sql.22P02", "Invalid array upper bound"))?;
        if stop < start {
            return invalid();
        }
        lower.push(start);
        lengths.push((i64::from(stop) - i64::from(start) + 1) as usize);
        value = &value[end + 1..];
    }
    if !lower.is_empty() {
        value = value
            .strip_prefix('=')
            .ok_or_else(|| error("sql.22P02", "Missing array dimension separator"))?;
    }
    Ok((value, lower, lengths))
}

/// Render nested dimensions from flat row-major element values.
fn render(dimensions: &[usize], values: &[Cell], position: &mut usize) -> Result<String> {
    if dimensions.is_empty() {
        return Ok("{}".into());
    }
    let mut children = Vec::new();
    for _ in 0..dimensions[0] {
        if dimensions.len() > 1 {
            children.push(render(&dimensions[1..], values, position)?);
        } else {
            let value = &values[*position];
            *position += 1;
            children.push(element_text(value)?);
        }
    }
    Ok("{".to_owned() + &children.join(",") + "}")
}
/// Classify malformed array text without silently flattening dimensions.
fn invalid<T>() -> Result<T> {
    Err(error("sql.22P02", "Malformed PostgreSQL array"))
}
/// Quote array elements only when PostgreSQL's text format needs delimiters/escapes.
fn element_text(value: &Cell) -> Result<String> {
    if value.is_null() {
        return Ok("NULL".into());
    }
    let value = super::operations::output(value)?;
    if value.is_empty()
        || value.eq_ignore_ascii_case("NULL")
        || value
            .chars()
            .any(|c| c.is_ascii_whitespace() || matches!(c, ',' | '{' | '}' | '"' | '\\'))
    {
        Ok("\"".to_owned() + &value.replace('\\', "\\\\").replace('"', "\\\"") + "\"")
    } else {
        Ok(value)
    }
}
impl PartialEq for ArrayValue {
    /// SQL array equality compares values and dimension/lower-bound metadata.
    fn eq(&self, right: &Self) -> bool {
        self.cmp(right) == Ordering::Equal
    }
}
impl Eq for ArrayValue {}
impl PartialOrd for ArrayValue {
    /// Use the PostgreSQL array total-order comparator.
    fn partial_cmp(&self, right: &Self) -> Option<Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for ArrayValue {
    /// Compare row-major elements, NULLs last, then dimension metadata.
    fn cmp(&self, right: &Self) -> Ordering {
        if self.element != right.element {
            return self.element.cmp(&right.element);
        }
        for (left, right) in self.values.iter().zip(&right.values) {
            let order = match (left.is_null(), right.is_null()) {
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (true, true) => Ordering::Equal,
                _ => {
                    super::operations::compare_sql(left, right).unwrap_or_else(|_| left.cmp(right))
                }
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.values
            .len()
            .cmp(&right.values.len())
            .then(self.dimensions.len().cmp(&right.dimensions.len()))
            .then(self.dimensions.cmp(&right.dimensions))
            .then(self.lower.cmp(&right.lower))
    }
}
