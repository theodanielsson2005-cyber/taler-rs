//! Unguessable order ID helpers (privacy default).

use crate::error::MerchantError;

/// Minimum encoded length accepted when `create_token` is false.
///
/// Shorter IDs are treated as enumerable unless a claim token is used.
pub const MIN_UNGUESSABLE_ORDER_ID_LEN: usize = 22;

/// Generate a high-entropy order ID suitable for public-ish references.
///
/// Prefer letting the client choose an unguessable ID (and setting
/// `create_token: false`) so claim tokens are unnecessary and order URLs
/// are not enumerable.
///
/// Format: `o-` + 32 hex chars (128 bits of randomness).
pub fn generate_order_id() -> Result<String, MerchantError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| MerchantError::Config(format!("failed to generate order id entropy: {e}")))?;
    let mut out = String::with_capacity(2 + 32);
    out.push_str("o-");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    Ok(out)
}

/// Returns true if `order_id` is acceptable when claim tokens are disabled.
///
/// Accepted forms:
/// - Canonical generator output: `o-` + 32 lowercase/uppercase hex digits
/// - Any id with length ≥ [`MIN_UNGUESSABLE_ORDER_ID_LEN`] using only
///   `A-Za-z0-9._-`, and not purely numeric
pub fn is_unguessable_order_id(order_id: &str) -> bool {
    validate_unguessable_order_id(order_id).is_ok()
}

/// Validate an order id for use with `create_token: false`.
pub fn validate_unguessable_order_id(order_id: &str) -> Result<(), MerchantError> {
    let id = order_id.trim();
    if id.is_empty() {
        return Err(MerchantError::WeakOrderId {
            order_id: order_id.to_string(),
            reason: "order_id must not be empty",
        });
    }

    if is_canonical_generated_id(id) {
        return Ok(());
    }

    if id.len() < MIN_UNGUESSABLE_ORDER_ID_LEN {
        return Err(MerchantError::WeakOrderId {
            order_id: id.to_string(),
            reason: "order_id too short for create_token=false (need ≥22 chars or o-+32 hex)",
        });
    }

    if id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MerchantError::WeakOrderId {
            order_id: id.to_string(),
            reason: "purely numeric order_id is enumerable",
        });
    }

    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(MerchantError::WeakOrderId {
            order_id: id.to_string(),
            reason: "order_id contains characters outside [A-Za-z0-9._-]",
        });
    }

    Ok(())
}

fn is_canonical_generated_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("o-") else {
        return false;
    };
    rest.len() == 32 && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generates_unique_hex_ids() {
        let mut set = HashSet::new();
        for _ in 0..32 {
            let id = generate_order_id().unwrap();
            assert!(id.starts_with("o-"));
            assert_eq!(id.len(), 34);
            assert!(id[2..].chars().all(|c| c.is_ascii_hexdigit()));
            assert!(validate_unguessable_order_id(&id).is_ok());
            assert!(set.insert(id));
        }
    }

    #[test]
    fn rejects_guessable_ids() {
        assert!(validate_unguessable_order_id("42").is_err());
        assert!(validate_unguessable_order_id("order-1").is_err());
        assert!(validate_unguessable_order_id("1234567890123456789012").is_err());
        assert!(validate_unguessable_order_id("").is_err());
    }

    #[test]
    fn accepts_long_slug() {
        assert!(validate_unguessable_order_id("shop-checkout-9f3a2c1b0e7d").is_ok());
    }
}
