//! Explicit builtin NULL behavior prevents unsupported placeholders from returning guessed NULLs.

/// Whether a function may use ordinary strict-argument SQL NULL propagation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NullPolicy {
    /// Any NULL argument produces a typed SQL NULL without executing the body.
    Strict,
    /// The body implements argument-specific NULL behavior.
    Custom,
}

/// Scalar-body capability metadata; overload resolution and volatility belong to the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinBehavior {
    /// Verified NULL behavior of this function family.
    pub null_policy: NullPolicy,
    /// Whether this implementation has a callable body, rather than only a known result type.
    pub implemented: bool,
}

/// Return scalar capability metadata independently of particular argument values.
pub fn builtin_behavior(name: &str) -> Option<BuiltinBehavior> {
    let (null_policy, implemented) = match name {
        "concat" | "concat_ws" | "array_append" | "array_prepend" | "string_to_array"
        | "array_to_string" | "json_build_object" | "json_build_array" | "jsonb_build_object"
        | "jsonb_build_array" => (NullPolicy::Custom, true),
        "make_interval" | "array_length" | "cardinality" | "jsonb_set" | "jsonb_typeof"
        | "jsonb_exists" | "jsonb_exists_any" | "jsonb_exists_all" | "get_bit" | "length"
        | "char_length" | "character_length" | "octet_length" | "substring" | "substr" | "left"
        | "right" | "lower" | "upper" | "replace" | "abs" | "round" | "extract" | "date_part" => {
            (NullPolicy::Strict, true)
        }
        "split_part" | "trunc" | "jsonb_strip_nulls" | "nextval" | "pg_terminate_backend" => {
            (NullPolicy::Strict, false)
        }
        "array_cat" => (NullPolicy::Custom, false),
        _ => return None,
    };
    Some(BuiltinBehavior {
        null_policy,
        implemented,
    })
}
