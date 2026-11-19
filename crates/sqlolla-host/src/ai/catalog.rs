//! List the models a BYOT key can reach, so a model choice is checked against live availability.
use super::*;

/// Ask the selected provider which text models this key can use; only the key leaves the host.
pub(super) async fn list(headers: HeaderMap) -> Result<Json<Value>> {
    let byot = Byot::from_headers(&headers)?;
    let catalog = provider::catalog(byot).await?;
    let models = provider::catalog_ids(byot.provider, &catalog)?;
    Ok(Json(envelope(byot.provider, models)))
}

/// Label the listing as provider-reported availability, distinct from SQLolla's tested default.
fn envelope(provider: Provider, models: Vec<String>) -> Value {
    let (provider, recommended) = provider.identity();
    json!({"schema":"sqlolla.ai.models.v1","status":"ok","provider":provider,
        "recommended":recommended,"models":models,
        "claim":"Listed by the provider for this key. Only the recommended model is tested with SQLolla's structured requests."})
}
