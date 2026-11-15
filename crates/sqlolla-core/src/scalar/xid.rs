//! Nominal transaction identities retain raw unsigned32 bits without numeric casts or SQL order.
use super::rich::Key;
use crate::json::error;
use crate::{Cell, Result, SqlType};
use std::cmp::Ordering;

/// PostgreSQL xidin and oidin share uint32in_subr; preserve the same portable input profile.
pub(super) fn input_key(text: &str) -> Result<(Key, String)> {
    let value = super::catalog::oid(text).map_err(|mut failure| {
        if failure.code == "unsupported.oid_negative_profile" {
            failure.code = "unsupported.xid_negative_profile".into();
        }
        failure
    })?;
    Ok((Key::Integer(i64::from(value)), value.to_string()))
}

/// Return bits only for a non-NULL nominal xid, never by implicitly casting an integer or OID.
pub fn xid_bits(value: &Cell) -> Result<u32> {
    if let Cell::Scalar(value) = value {
        if value.ty == SqlType::Xid {
            if let Key::Integer(bits) = value.key {
                return u32::try_from(bits)
                    .map_err(|_| error("invalid.xid_bits", "Xid bits outside unsigned32"));
            }
        }
    }
    Err(error(
        "invalid.xid_value",
        "Expected a non-NULL nominal xid",
    ))
}

/// Only xid-left/int4-right has the measured PostgreSQL mixed equality overload.
pub(super) fn mixed_comparison(left: &Cell, right: &Cell) -> Result<Option<Ordering>> {
    if left.sql_type() == SqlType::Xid {
        if let Cell::Int32(right) = right {
            return Ok(Some(xid_bits(left)?.cmp(&(*right as u32))));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Xid identity remains separate from OID/int4 despite sharing its raw numeric spelling.
    #[test]
    fn nominal_identity_and_signed_bits() {
        let xid = crate::scalar::input(&SqlType::Xid, "-1").unwrap();
        assert_eq!(xid_bits(&xid).unwrap(), u32::MAX);
        assert_eq!(
            mixed_comparison(&xid, &Cell::Int32(-1)).unwrap(),
            Some(Ordering::Equal)
        );
        assert_ne!(
            xid,
            crate::scalar::input(&SqlType::Oid, "4294967295").unwrap()
        );
        assert_eq!(
            crate::scalar::cast(&Cell::Int32(1), &SqlType::Xid)
                .unwrap_err()
                .code,
            "sql.42846"
        );
    }

    /// Shared uint32 input refuses platform-dependent overflow/suffix precedence explicitly.
    #[test]
    fn overflow_suffix_is_not_a_guessed_sqlstate() {
        for ty in [SqlType::Oid, SqlType::Xid] {
            for input in ["42949672960x", "0x100000000x", "040000000000_1"] {
                assert_eq!(
                    crate::scalar::input(&ty, input).unwrap_err().code,
                    "unsupported.catalog_overflow_profile"
                );
            }
            assert_eq!(
                crate::scalar::input(&ty, "4294967296").unwrap_err().code,
                "sql.22003"
            );
            assert_eq!(
                crate::scalar::input(&ty, "1x").unwrap_err().code,
                "sql.22P02"
            );
        }
    }
}
