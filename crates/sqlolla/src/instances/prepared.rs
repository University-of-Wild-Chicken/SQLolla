//! Regenerate compiler artifacts and replay each decoded solver instance before publication.
use super::*;
use crate::alloy::{self, Encoded, Inputs, Property};
use crate::bridge;
use crate::finite::{self, FiniteModel};
use sqlolla_core::{Column, ExecutionContext, StatementPlan};

/// The two supported encodings retain distinct semantic assurance descriptions.
pub(super) enum Model {
    Native(Encoded),
    Finite(FiniteModel),
}

/// Rebuilt artifacts are never loaded as trusted executable plans from archive JSON.
pub(super) struct Prepared {
    pub request: Request,
    pub statement: StatementPlan,
    pub context: ExecutionContext,
    pub model: Model,
}

impl Prepared {
    /// Parse and bind exact stored SQL, then reconstruct the deterministic encoding inventory.
    pub(super) fn new(pipeline: &mut Pipeline, request: Request) -> Outcome<Self> {
        Self::prepare(pipeline, request, None)
    }

    /// Bind candidate SQL under the archived request and require its exact output positions.
    pub(super) fn candidate(
        pipeline: &mut Pipeline,
        bundle: &Bundle,
        sql: String,
    ) -> Outcome<Self> {
        let request = Request {
            sql,
            ..bundle.request.clone()
        };
        Self::prepare(pipeline, request, Some(&bundle.output_schema))
    }

    /// Reject a changed output contract before constructing a bounded model.
    fn prepare(
        pipeline: &mut Pipeline,
        mut request: Request,
        schema: Option<&[Column]>,
    ) -> Outcome<Self> {
        request.id = Value::Null;
        request.include_model = false;
        request.validate()?;
        pipeline.prepare_metadata(&mut request)?;
        let ast = pipeline.parse(&request.sql)?;
        let statement = sqlolla_core::bind_statement_with_source(
            &ast,
            &request.sql,
            &request.catalog,
            &request.parameters,
        )
        .map_err(|error| (error.code, error.message))?;
        require_schema(schema, &statement)?;
        let context = crate::extended::catalog_context(&request)?;
        let model = build(&request, &statement, &context)?;
        Ok(Self {
            request,
            statement,
            context,
            model,
        })
    }

    /// Expose exact compiler output without allowing user edits to bypass regeneration.
    pub(super) fn source(&self) -> &str {
        match &self.model {
            Model::Native(model) => &model.model,
            Model::Finite(model) => &model.model,
        }
    }

    /// Select the IR form corresponding to the recorded encoder profile.
    pub(super) fn ir(&self) -> Value {
        match (&self.model, &self.statement) {
            (Model::Native(_), StatementPlan::Query(plan)) => json!(plan),
            _ => json!(self.statement),
        }
    }

    /// Native RA and evaluator-backed tables must never share an assurance label.
    pub(super) fn assurance(&self) -> &'static str {
        match self.model {
            Model::Native(_) => "native_ra_alloy_with_rust_replay",
            Model::Finite(_) => "evaluator_backed_table_with_rust_replay",
        }
    }

    /// Decode and replay both examples and counterexamples against the same bound semantics.
    pub(super) fn replay(&self, response: &Value, kind: Kind) -> Outcome<Value> {
        match &self.model {
            Model::Native(model) => native_replay(self, model, response, kind),
            Model::Finite(model) => self.finite_replay(model, response, kind),
        }
    }

    /// A finite-table selection must retain the complete typed transition, including errors/effects.
    fn finite_replay(&self, model: &FiniteModel, response: &Value, kind: Kind) -> Outcome<Value> {
        let index = model.decode(response).map_err(engine)?;
        let transition = &model.transitions[index];
        crate::extended::replay(&self.statement, &self.context, transition)?;
        if kind == Kind::Counterexample
            && !crate::extended::evidence::violated(property(&self.request)?, &transition.outcome)
        {
            return Err(failure(
                "evidence.replay",
                "Selected transition does not violate property",
            ));
        }
        Ok(
            json!({"database":transition.input,"catalog":transition.input_catalog,
            "outcome":transition.outcome,"transition_index":index}),
        )
    }
}

/// Reject a changed required column contract without encoding the candidate.
fn require_schema(expected: Option<&[Column]>, statement: &StatementPlan) -> Outcome<()> {
    if expected.is_some_and(|expected| expected != crate::extended::schema(statement)) {
        return Err(failure(
            "refinement.output_schema",
            "Candidate changes the archived output schema",
        ));
    }
    Ok(())
}

/// Reuse exactly the profile routing and complete finite transition builder from normal analysis.
fn build(
    request: &Request,
    statement: &StatementPlan,
    context: &ExecutionContext,
) -> Outcome<Model> {
    if let StatementPlan::Query(plan) = statement
        && crate::pipeline::native_profile(request, plan)
    {
        let input = if request.mode == "analyze" {
            Inputs::Symbolic {
                bounds: &request.bounds,
                domain: &request.domain,
            }
        } else {
            Inputs::Concrete(&request.database)
        };
        return alloy::encode(plan, input, request.property.as_ref())
            .map(Model::Native)
            .map_err(crate::pipeline::encoding);
    }
    let transitions = crate::extended::transitions(request, statement, context)?;
    finite::encode(
        transitions,
        crate::extended::schema(statement),
        request.property.as_ref(),
    )
    .map(Model::Finite)
    .map_err(crate::pipeline::encoding)
}

/// Native witnesses independently decode table slots and compare the Rust result bag.
fn native_replay(
    prepared: &Prepared,
    model: &Encoded,
    response: &Value,
    kind: Kind,
) -> Outcome<Value> {
    let StatementPlan::Query(plan) = &prepared.statement else {
        return Err(engine("Native statement mismatch".into()));
    };
    let database = complete_input(prepared, model, response)?;
    let output = bridge::decode(model, response, &model.output).map_err(engine)?;
    let mut replay =
        sqlolla_core::eval(plan, &database).map_err(|error| (error.code, error.message))?;
    replay.sort();
    if output != replay {
        return Err(failure(
            "evidence.replay",
            "Native Alloy/Rust result mismatch",
        ));
    }
    if kind == Kind::Counterexample && !alloy::violated(property(&prepared.request)?, &replay) {
        return Err(failure(
            "evidence.replay",
            "Selected native model does not violate property",
        ));
    }
    Ok(json!({"database":database,"catalog":prepared.request.catalog,"output":output}))
}

/// Unreferenced concrete tables retain supplied values; symbolic omitted tables select empty bags.
fn complete_input(
    prepared: &Prepared,
    model: &Encoded,
    response: &Value,
) -> Outcome<sqlolla_core::Database> {
    let mut database = bridge::database(model, response).map_err(engine)?;
    for table in prepared.request.catalog.tables.keys() {
        database.entry(table.clone()).or_insert_with(|| {
            prepared
                .request
                .database
                .get(table)
                .cloned()
                .unwrap_or_default()
        });
    }
    sqlolla_core::validate_database(&prepared.request.catalog, &database)
        .map_err(|error| (error.code, error.message))?;
    Ok(database)
}

/// Counterexample labels require an explicit archived property, independently of command names.
fn property(request: &Request) -> Outcome<&Property> {
    request
        .property
        .as_ref()
        .ok_or_else(|| failure("evidence.property", "Counterexample needs property"))
}
