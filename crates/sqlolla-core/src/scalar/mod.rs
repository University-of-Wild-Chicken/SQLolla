//! Deterministic scalar values, validated PostgreSQL input and pure operations.
mod array_functions;
mod arrays;
mod behavior;
mod catalog;
mod collections;
mod constructor;
mod decimal;
mod floating;
mod functions;
mod input;
mod interval;
mod interval_format;
mod interval_parse;
mod json_access;
mod json_functions;
mod json_update;
mod json_value;
mod operations;
mod regex;
mod rich;
mod string_collections;
mod temporal;
mod typing;
mod xid;
pub use behavior::{builtin_behavior, BuiltinBehavior, NullPolicy};
pub use constructor::{array_constructor, array_elements};
pub use decimal::Decimal;
pub use floating::PgFloat;
pub use functions::function;
pub use input::input;
pub use interval::{interval_cast, interval_input, IntervalFields, IntervalQualifier};
pub use operations::{arithmetic, assignment_cast, cast, compare_sql, duplicate_eq, output};
pub use regex::regex_match;
pub use rich::ScalarValue;
pub use typing::{common_type, comparison_type, compatible, operation_type, return_type};
pub use xid::xid_bits;
#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod interval_tests;
#[cfg(test)]
mod tests;
