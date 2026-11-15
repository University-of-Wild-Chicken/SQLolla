//! Alloy worker invocation and type-aware positional instance decoding.
use crate::alloy::{Encoded, Layout};
use crate::process::{Worker, root, tool};
use serde_json::{Value, json};
use sqlolla_core::{Bag, Cell, Database, Row, SqlType};
use std::collections::BTreeMap;
use std::process::Command;

/// Private Java API bridge; it does not decide SQL properties.
pub struct Bridge {
    worker: Worker,
}

impl Bridge {
    /// Start the pinned Java17 worker with an explicit heap limit and classpath.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            worker: Worker::spawn(bridge_command()?)?,
        })
    }

    /// Validate the bridge protocol and reject every error as an engine failure.
    pub fn solve(&mut self, model: &str, command: usize) -> Result<Value, String> {
        self.request(json!({"model": model, "command": command}))
    }

    /// Start an on-demand Alloy API enumeration with an explicit command identity.
    pub fn begin(&mut self, model: &str, command: usize) -> Result<Value, String> {
        self.request(json!({"action":"begin","model":model,"command":command}))
    }

    /// Advance exactly one retained Alloy solution; UNSAT alone establishes exhaustion.
    pub fn next_solution(&mut self) -> Result<Value, String> {
        self.request(json!({"action":"next"}))
    }

    /// Strict transport handling keeps engine failures outside the SAT/UNSAT channel.
    fn request(&mut self, request: Value) -> Result<Value, String> {
        let request = request.to_string();
        let response = self.worker.request(&request)?;
        let result: Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;
        if result["protocol"] != "sqlolla.alloy.v1" {
            return Err("invalid bridge protocol".into());
        }
        match result["status"].as_str() {
            Some("SAT" | "UNSAT") => Ok(result),
            _ => Err(format!("Alloy engine error: {}", result["message"])),
        }
    }

    /// Require an actual Alloy assertion check, not a satisfiability run relabeled as a check.
    pub fn check(&mut self, model: &str, command: usize) -> Result<Value, String> {
        let result = self.solve(model, command)?;
        if result["command_kind"] != "check" || result["command_label"] != "QueryProperty" {
            return Err("property command is not the expected Alloy assertion check".into());
        }
        Ok(result)
    }
}

/// Resolve the native JVM and classpath without passing either through a shell.
fn bridge_command() -> Result<Command, String> {
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    let java = tool(
        "SQLolla_JAVA",
        format!("m0/alloy/.tools/jdk-17.0.21+9/bin/{binary}"),
    );
    let jar = tool(
        "SQLolla_ALLOY_JAR",
        "../dependencies/org.alloytools.alloy.dist.jar",
    );
    let classpath =
        std::env::join_paths([root().join("bridge/classes"), jar]).map_err(|e| e.to_string())?;
    let mut command = Command::new(java);
    command.args([
        "-Xmx512m",
        "-Dfile.encoding=UTF-8",
        "-Djava.awt.headless=true",
        "-cp",
    ]);
    command.arg(classpath).arg("AlloyBridge");
    Ok(command)
}

/// Decode live output occurrences with ordered cells and resolved SQL NULL types.
pub fn decode(encoded: &Encoded, response: &Value, layout: &Layout) -> Result<Bag, String> {
    let relations = response["relations"]
        .as_object()
        .ok_or("missing instance relations")?;
    let value_atoms = value_atoms(encoded, relations)?;
    let alive = relation(relations, &format!("this/{}.alive", layout.name))?;
    let mut bag = Vec::new();
    for slot in &layout.rows {
        let atom = singleton(relations, &format!("this/{slot}"))?;
        let live = field_value(&alive, &atom)?;
        if !live_flag(&live, &value_atoms)? {
            continue;
        }
        bag.push(decode_row(layout, relations, &atom, &value_atoms)?);
    }
    bag.sort();
    Ok(bag)
}

/// Refuse unknown or NULL liveness values instead of silently treating corruption as false.
fn live_flag(atom: &str, values: &BTreeMap<String, Cell>) -> Result<bool, String> {
    match values.get(atom) {
        Some(Cell::Bool(value)) => Ok(*value),
        _ => Err("invalid row liveness value".into()),
    }
}

/// Decode one live occurrence's cells in declared output-column order.
fn decode_row(
    layout: &Layout,
    relations: &serde_json::Map<String, Value>,
    atom: &str,
    values: &BTreeMap<String, Cell>,
) -> Result<Row, String> {
    layout
        .schema
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let field = relation(relations, &format!("this/{}.c{index}", layout.name))?;
            let value = field_value(&field, atom)?;
            decode_cell(&value, column.ty.clone(), values)
        })
        .collect()
}

/// Decode the same shared input relations for independent Rust witness replay.
pub fn database(encoded: &Encoded, response: &Value) -> Result<Database, String> {
    encoded
        .inputs
        .iter()
        .map(|(name, layout)| Ok((name.clone(), decode(encoded, response, layout)?)))
        .collect()
}

/// Resolve singleton scalar atoms from the API rather than guessing `$0` suffixes.
fn value_atoms(
    encoded: &Encoded,
    relations: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<String, Cell>, String> {
    let mut values = BTreeMap::new();
    for (name, cell) in &encoded.values {
        values.insert(singleton(relations, &format!("this/{name}"))?, cell.clone());
    }
    values.insert(
        singleton(relations, "this/Null")?,
        Cell::Null(SqlType::Bool),
    );
    Ok(values)
}

/// Retype the universal encoding NULL at its declared output column.
fn decode_cell(atom: &str, ty: SqlType, values: &BTreeMap<String, Cell>) -> Result<Cell, String> {
    match values.get(atom).ok_or("unknown scalar atom")? {
        Cell::Null(_) => Ok(Cell::Null(ty)),
        value if super::alloy::type_of(value) == ty => Ok(value.clone()),
        _ => Err("decoded scalar type mismatch".into()),
    }
}

/// Read a typed relation while preserving tuple arity and position.
fn relation(
    relations: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Vec<Vec<String>>, String> {
    serde_json::from_value(
        relations
            .get(key)
            .ok_or(format!("missing relation {key}"))?
            .clone(),
    )
    .map_err(|e| e.to_string())
}

/// Require the precise singleton shape expected by a compiler-created signature.
fn singleton(relations: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    let tuples = relation(relations, key)?;
    match tuples.as_slice() {
        [row] if row.len() == 1 => Ok(row[0].clone()),
        _ => Err(format!("non-singleton {key}")),
    }
}

/// Require one scalar assignment for a given row; missing or duplicate tuples are errors.
fn field_value(tuples: &[Vec<String>], row: &str) -> Result<String, String> {
    if tuples.iter().any(|tuple| tuple.len() != 2) {
        return Err("invalid field tuple arity".into());
    }
    let matches: Vec<_> = tuples
        .iter()
        .filter(|t| t.len() == 2 && t[0] == row)
        .collect();
    match matches.as_slice() {
        [tuple] => Ok(tuple[1].clone()),
        _ => Err("invalid functional field".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{field_value, live_flag};
    use sqlolla_core::{Cell, SqlType};
    use std::collections::BTreeMap;

    /// Corrupt or missing liveness atoms must not make a row disappear silently.
    #[test]
    fn rejects_invalid_liveness() {
        let values = BTreeMap::from([("null".into(), Cell::Null(SqlType::Bool))]);
        assert!(live_flag("null", &values).is_err());
        assert!(live_flag("absent", &values).is_err());
    }

    /// Reject malformed extra tuples even if the requested row has a valid assignment.
    #[test]
    fn rejects_malformed_field_tuples() {
        let tuples = vec![vec!["row".into(), "value".into()], vec!["corrupt".into()]];
        assert!(field_value(&tuples, "row").is_err());
    }
}
