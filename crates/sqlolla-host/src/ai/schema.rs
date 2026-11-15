//! Strict provider output shape complements, but never replaces, local business-rule validation.
use super::*;

/// Dynamic identities and the exact supplied fact inventory constrain every structured response.
pub(super) fn response(input: &Input, prompt: &context::Prompt, proposal: bool) -> Value {
    let refs: Vec<_> = prompt.references.iter().collect();
    let reference = if refs.is_empty() {
        json!({"type":"string"})
    } else {
        json!({"type":"string","enum":refs})
    };
    let claim = json!({"type":"object","additionalProperties":false,"required":["text","fact_references"],
        "properties":{"text":{"type":"string","maxLength":1200},
            "fact_references":{"type":"array","items":reference,"minItems":1,"maxItems":8}}});
    json!({"type":"object","additionalProperties":false,
        "required":["instance_id","model_revision_sha256","context_sha256","title","claims","assumptions","sql"],
        "properties":{"instance_id":{"type":"string","enum":[input.instance_id]},
            "model_revision_sha256":{"type":"string","enum":[input.model_revision_sha256]},
            "context_sha256":{"type":"string","enum":[prompt.hash]},
            "title":{"type":"string","maxLength":160},
            "claims":{"type":"array","items":claim,"maxItems":if refs.is_empty(){0}else{8}},
            "assumptions":{"type":"array","items":{"type":"string","maxLength":512},"maxItems":8},
            "sql":if proposal {json!({"type":"string","minLength":1,"maxLength":16384})} else {json!({"type":"null"})}}})
}
