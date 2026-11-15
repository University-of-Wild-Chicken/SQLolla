//! Resolve declared functions jointly with visible PostgreSQL builtin candidates.
use super::*;
use crate::function_registry::{
    builtin_name, builtin_signatures, implicit_cast, nominal, search_path,
};
#[cfg(test)]
mod tests;

/// Candidate signatures remain separate from callable body capabilities.
struct Candidate<'a> {
    function: Option<&'a ImmutableSqlFunction>,
    parameters: Vec<SqlType>,
}

/// Resolve signatures before contextualizing unknown literals or checking execution permissions.
pub(super) fn resolve<'a>(
    expressions: &Expressions<'_>,
    functions: &'a SqlFunctions,
    node: &Value,
) -> Result<Option<&'a ImmutableSqlFunction>> {
    let parts = identifiers(node)?;
    let (schemas, name) = match parts.as_slice() {
        [name] => (search_path(&functions.search_path), *name),
        [schema, name] => (vec![*schema], *name),
        _ => {
            return Err(error(
                "unsupported.function_namespace",
                "Unsupported qualified function identity",
            ))
        }
    };
    if !functions
        .definitions
        .iter()
        .any(|f| f.name == name && schemas.contains(&f.schema.as_str()))
    {
        return Ok(None);
    }
    let arguments = array(node, "args")?
        .iter()
        .map(|arg| expressions.hint(arg))
        .collect::<Result<Vec<_>>>()?;
    let candidates = candidates(functions, &schemas, name, arguments.len())?;
    select(candidates, &arguments).map(|candidate| candidate.function)
}

/// Earlier schemas hide only identical nominal signatures, never differently typed overloads.
fn candidates<'a>(
    functions: &'a SqlFunctions,
    schemas: &[&str],
    name: &str,
    arity: usize,
) -> Result<Vec<Candidate<'a>>> {
    let mut result: Vec<Candidate<'a>> = Vec::new();
    for schema in schemas {
        let additions = schema_candidates(functions, schema, name)?;
        for candidate in additions
            .into_iter()
            .filter(|c| c.parameters.len() == arity)
        {
            if !result
                .iter()
                .any(|old| old.parameters == candidate.parameters)
            {
                result.push(candidate);
            }
        }
    }
    Ok(result)
}

/// Unsupported builtin collision families are refused instead of inventing their signatures.
fn schema_candidates<'a>(
    functions: &'a SqlFunctions,
    schema: &str,
    name: &str,
) -> Result<Vec<Candidate<'a>>> {
    if schema == "pg_catalog" && builtin_name(name) {
        return builtin_signatures(name).map(|signatures| {
            signatures
                .into_iter()
                .map(|parameters| Candidate {
                    function: None,
                    parameters,
                })
                .collect()
        });
    }
    Ok(functions
        .definitions
        .iter()
        .filter(|f| f.schema == schema && f.name == name)
        .map(|function| Candidate {
            function: Some(function),
            parameters: function.parameters.iter().map(|p| nominal(&p.ty)).collect(),
        })
        .collect())
}

/// An exact match wins; unique compatible candidates are admitted without guessing tie-break rules.
fn select<'a>(
    mut candidates: Vec<Candidate<'a>>,
    arguments: &[Option<SqlType>],
) -> Result<Candidate<'a>> {
    if candidates
        .iter()
        .any(|candidate| exact(candidate, arguments))
    {
        candidates.retain(|candidate| exact(candidate, arguments));
    } else {
        candidates.retain(|candidate| compatible(candidate, arguments));
    }
    match candidates.len() {
        0 => Err(error(
            "sql.42883",
            "No function overload accepts these argument types",
        )),
        1 => Ok(candidates.remove(0)),
        _ => Err(error(
            "unsupported.function_resolution",
            "Multiple coercible candidates require additional PostgreSQL ranking rules",
        )),
    }
}

/// Unknown literals never constitute an exact overload match.
fn exact(candidate: &Candidate<'_>, arguments: &[Option<SqlType>]) -> bool {
    candidate
        .parameters
        .iter()
        .zip(arguments)
        .all(|(target, source)| {
            source
                .as_ref()
                .is_some_and(|source| nominal(source) == *target)
        })
}

/// Unknown literals can be contextualized only after candidate selection.
fn compatible(candidate: &Candidate<'_>, arguments: &[Option<SqlType>]) -> bool {
    candidate
        .parameters
        .iter()
        .zip(arguments)
        .all(|(target, source)| {
            source
                .as_ref()
                .is_none_or(|source| implicit_cast(source, target))
        })
}
