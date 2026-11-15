//! Bind complete admitted table declarations and generate collision-aware constraint names.
use super::*;
use serde_json::json;

/// All upstream declaration flags are checked, including defaults and independent key spelling.
const COLUMN_FIELDS: &[&str] = &[
    "name",
    "type",
    "nullable",
    "default",
    "primary_key",
    "unique",
];

impl Binder<'_> {
    /// An occupied IF NOT EXISTS target skips declaration analysis, matching PostgreSQL.
    pub(super) fn create_table(&self, node: &Value) -> Result<CreateTablePlan> {
        fields(
            node,
            &["tag", "name", "if_not_exists", "columns", "constraints"],
        )?;
        let name = identifier(text(node, "name")?)?;
        let schema = creation_schema(self.catalog, None)?;
        let if_not_exists = boolean(node, "if_not_exists")?;
        let skip_existing = occupied(self.catalog, &schema, &name)?;
        if skip_existing && !if_not_exists {
            return Err(error("sql.42P07", "Relation already exists"));
        }
        let mut plan = empty_plan(schema, name, if_not_exists, skip_existing);
        if skip_existing {
            return Ok(plan);
        }
        self.table_declarations(node, &mut plan)?;
        record_key_order(&mut plan);
        Ok(plan)
    }

    /// Bind declarations before resolving table-level and column-level key positions.
    fn table_declarations(&self, node: &Value, plan: &mut CreateTablePlan) -> Result<()> {
        let declarations = array(node, "columns")?;
        for declaration in declarations {
            self.declare_column(declaration, plan)?;
        }
        unique_names(
            &plan
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>(),
        )?;
        for constraint in array(node, "constraints")? {
            table_key(constraint, plan, self.catalog)?;
        }
        for (index, column) in declarations.iter().enumerate() {
            if boolean(column, "primary_key")? {
                add_key(plan, vec![index], true, self.catalog)?;
            }
            if boolean(column, "unique")? {
                add_key(plan, vec![index], false, self.catalog)?;
            }
        }
        Ok(())
    }

    /// Bind storage type and literal defaults without evaluating arbitrary default expressions.
    fn declare_column(&self, node: &Value, plan: &mut CreateTablePlan) -> Result<()> {
        fields(node, COLUMN_FIELDS)?;
        let name = identifier(text(node, "name")?)?;
        let cast = json!({"tag":"cast","arg":{"tag":"sql_null"},"type":node["type"]});
        let ty = self.expressions(&[]).bind(&cast, None)?.ty;
        let nullable = match &node["nullable"] {
            Value::Null => true,
            Value::Bool(value) => *value,
            _ => return Err(error("invalid.ast", "Invalid nullable flag")),
        };
        if !node["default"].is_null() {
            let value = self.literal_default(&node["default"], &ty)?;
            plan.semantics
                .defaults
                .insert(plan.columns.len(), DefaultSpec::Literal { value });
        }
        plan.columns.push(Column {
            name,
            ty,
            nullable,
            qualifier: None,
            schema: None,
        });
        Ok(())
    }

    /// Literal default storage is safe to persist without fabricating missing expression source spans.
    fn literal_default(&self, node: &Value, ty: &SqlType) -> Result<Cell> {
        if !matches!(
            text(node, "tag")?,
            "integer" | "decimal" | "text" | "boolean" | "sql_null"
        ) {
            return Err(error(
                "unsupported.ddl_default",
                "Only literal defaults have a complete persistent representation",
            ));
        }
        let expression = self.expressions(&[]).assignment(node, ty)?;
        eval_expr(&expression, &vec![])
    }
}

/// PostgreSQL creates the primary-key index before the remaining unique constraint indexes.
fn record_key_order(plan: &mut CreateTablePlan) {
    plan.semantics.unique_enforcement_order = plan
        .semantics
        .primary_key
        .iter()
        .chain(&plan.semantics.unique)
        .map(|key| key.name.clone())
        .collect();
}

/// A skipped declaration carries no fabricated columns or constraints.
fn empty_plan(
    schema: String,
    name: String,
    if_not_exists: bool,
    skip_existing: bool,
) -> CreateTablePlan {
    CreateTablePlan {
        schema,
        name,
        columns: vec![],
        semantics: TableSemantics {
            complete: true,
            ..Default::default()
        },
        if_not_exists,
        skip_existing,
    }
}

/// Resolve every named key position and reject missing or repeated columns.
fn table_key(node: &Value, plan: &mut CreateTablePlan, catalog: &Catalog) -> Result<()> {
    fields(node, &["tag", "columns"])?;
    let primary = match text(node, "tag")? {
        "primary_key" => true,
        "unique" => false,
        tag => return Err(error("unsupported.ddl_constraint", tag)),
    };
    let columns = array(node, "columns")?
        .iter()
        .map(|name| {
            let name = name
                .as_str()
                .ok_or_else(|| error("invalid.ast", "Nontext key column"))?;
            plan.columns
                .iter()
                .position(|column| column.name == name)
                .ok_or_else(|| error("sql.42703", "Key column does not exist"))
        })
        .collect::<Result<Vec<_>>>()?;
    add_key(plan, columns, primary, catalog)
}

/// Install one immediate key and preserve primary-key NOT NULL implications.
fn add_key(
    plan: &mut CreateTablePlan,
    columns: Vec<usize>,
    primary: bool,
    catalog: &Catalog,
) -> Result<()> {
    validate_key(plan, &columns, primary)?;
    let name = key_name(plan, &columns, primary, catalog)?;
    let key = KeyConstraint {
        name,
        columns: columns.clone(),
        nulls_not_distinct: false,
        deferrable: false,
        initially_deferred: false,
    };
    if primary {
        for index in columns {
            plan.columns[index].nullable = false;
        }
        plan.semantics.primary_key = Some(key);
    } else {
        plan.semantics.unique.push(key);
    }
    Ok(())
}

/// Duplicate key/index merging remains an explicit coverage boundary rather than fabricated objects.
fn validate_key(plan: &CreateTablePlan, columns: &[usize], primary: bool) -> Result<()> {
    if columns.is_empty() || columns.iter().collect::<BTreeSet<_>>().len() != columns.len() {
        return Err(error("sql.42701", "Repeated key column"));
    }
    if primary && plan.semantics.primary_key.is_some() {
        return Err(error("sql.42P16", "Multiple primary keys"));
    }
    if plan
        .semantics
        .primary_key
        .iter()
        .chain(&plan.semantics.unique)
        .any(|key| key.columns == columns)
    {
        return Err(error(
            "unsupported.ddl_duplicate_keys",
            "PostgreSQL key/index merging needs an explicit naming contract",
        ));
    }
    Ok(())
}

/// Generated key names reserve occupied relation names and earlier keys in the same declaration.
fn key_name(
    plan: &CreateTablePlan,
    columns: &[usize],
    primary: bool,
    catalog: &Catalog,
) -> Result<String> {
    let base = key_base(plan, columns, primary);
    for number in 0..1000 {
        let candidate = identifier(&if number == 0 {
            base.clone()
        } else {
            format!("{base}{number}")
        })?;
        let used = plan
            .semantics
            .primary_key
            .iter()
            .chain(&plan.semantics.unique)
            .any(|key| key.name == candidate);
        if !used && candidate != plan.name && !occupied(catalog, &plan.schema, &candidate)? {
            return Ok(candidate);
        }
    }
    Err(error(
        "resource.ddl_names",
        "Constraint naming work budget exceeded",
    ))
}

/// Primary keys use a fixed suffix; unique keys include ordered column identities.
fn key_base(plan: &CreateTablePlan, columns: &[usize], primary: bool) -> String {
    let suffix = if primary {
        "pkey".into()
    } else {
        format!(
            "{}_key",
            columns
                .iter()
                .map(|index| plan.columns[*index].name.as_str())
                .collect::<Vec<_>>()
                .join("_")
        )
    };
    format!("{}_{}", plan.name, suffix)
}
