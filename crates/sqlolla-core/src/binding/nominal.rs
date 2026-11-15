//! Nominal type lookup uses catalog declarations rather than the current projected row shape.
use super::*;

/// Resolve exact nominal identity along the declared effective schema path.
pub(super) fn resolve(catalog: &Catalog, name: &str) -> Result<Option<SqlType>> {
    let candidates = if name.contains('.') {
        vec![name.to_owned()]
    } else {
        catalog
            .search_path
            .iter()
            .map(|schema| format!("{schema}.{name}"))
            .collect()
    };
    for candidate in candidates {
        let mut found = None;
        for column in catalog.tables.values().flat_map(|table| &table.columns) {
            if let Some(ty) = declared(&column.ty, &candidate) {
                if found.as_ref().is_some_and(|value| value != &ty) {
                    return Err(error(
                        "invalid.nominal_type",
                        "Conflicting declarations for one type identity",
                    ));
                }
                found = Some(ty);
            }
        }
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// An array exposes its element's nominal declaration without creating a new enum identity.
fn declared(ty: &SqlType, name: &str) -> Option<SqlType> {
    match ty {
        SqlType::Array(element) => declared(element, name),
        SqlType::Enum { name: identity, .. } if identity == name => Some(ty.clone()),
        _ => None,
    }
}
