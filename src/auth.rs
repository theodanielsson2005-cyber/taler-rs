//! Merchant backend authentication tokens with secret redaction.

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

const SECRET_TOKEN_PREFIX: &str = "secret-token:";

/// Access token for private Merchant Backend endpoints.
///
/// Stored without logging the secret. [`Display`] and [`Debug`] redact the
/// value. Memory is zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretToken {
    /// Raw token material after optional `secret-token:` normalization.
    inner: String,
}

impl SecretToken {
    /// Build from an env/CLI value.
    ///
    /// Accepts either `sandbox` or the full `secret-token:sandbox` form.
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let inner = if let Some(rest) = raw.strip_prefix(SECRET_TOKEN_PREFIX) {
            rest.to_string()
        } else {
            raw
        };
        Self { inner }
    }

    /// Value for the `Authorization` header (`Bearer secret-token:…`).
    ///
    /// Callers that hold this `String` should [`Zeroize::zeroize`] it after the
    /// HTTP call when feasible; the client does so for its own request path.
    pub fn authorization_header_value(&self) -> String {
        format!("Bearer {SECRET_TOKEN_PREFIX}{}", self.inner)
    }

    /// Returns true if no token material was provided.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl PartialEq for SecretToken {
    fn eq(&self, other: &Self) -> bool {
        // Equality helper for tests/config — not a cryptographic compare API.
        self.inner == other.inner
    }
}

impl Eq for SecretToken {}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([REDACTED])")
    }
}

impl fmt::Display for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Optional claim token returned when creating an order with `create_token: true`.
///
/// # Logging vs persistence
///
/// - [`Debug`] / [`Display`] always redact.
/// - [`Serialize`] also emits `"[REDACTED]"` so accidental `serde_json::to_string`
///   in logs cannot leak the token.
/// - [`Deserialize`] accepts the real wire value from the Merchant Backend.
/// - To **intentionally** persist or embed the secret (e.g. durable order store,
///   pay URI construction), use [`ClaimToken::as_str`] — never rely on `Serialize`.
///
/// Memory is zeroized on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ClaimToken(String);

impl ClaimToken {
    /// Wrap a claim token string from the API.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the raw claim token for intentional use (pay URI / durable store).
    ///
    /// **Do not** log this value. [`Serialize`] will not give you the secret back.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClaimToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClaimToken([REDACTED])")
    }
}

impl fmt::Display for ClaimToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Serialize for ClaimToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Intentional: serde must not be a silent secret channel.
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'de> Deserialize<'de> for ClaimToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ClaimTokenVisitor;
        impl Visitor<'_> for ClaimTokenVisitor {
            type Value = ClaimToken;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a claim token string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(ClaimToken::new(v))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(ClaimToken::new(v))
            }
        }
        deserializer.deserialize_string(ClaimTokenVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PostOrderResponse;

    #[test]
    fn normalizes_prefix() {
        let a = SecretToken::new("sandbox");
        let b = SecretToken::new("secret-token:sandbox");
        assert_eq!(
            a.authorization_header_value(),
            "Bearer secret-token:sandbox"
        );
        assert_eq!(
            a.authorization_header_value(),
            b.authorization_header_value()
        );
        assert_eq!(a, b);
    }

    #[test]
    fn redacts_debug_and_display() {
        let t = SecretToken::new("super-secret");
        let d = format!("{t:?}");
        let s = format!("{t}");
        assert!(!d.contains("super-secret"));
        assert!(!s.contains("super-secret"));
        assert!(d.contains("REDACTED"));
    }

    #[test]
    fn claim_token_redacted_in_debug_and_serde() {
        let c = ClaimToken::new("claim-xyz");
        assert!(!format!("{c:?}").contains("claim-xyz"));
        assert_eq!(c.as_str(), "claim-xyz");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"[REDACTED]\"");
        assert!(!json.contains("claim-xyz"));
        let wire: ClaimToken = serde_json::from_str("\"claim-xyz\"").unwrap();
        assert_eq!(wire.as_str(), "claim-xyz");
    }

    #[test]
    fn post_order_response_serialize_does_not_persist_claim_secret() {
        let resp = PostOrderResponse {
            order_id: "o-1".into(),
            token: Some(ClaimToken::new("claim-must-not-appear")),
            pay_deadline: crate::types::Timestamp {
                t_s: Some(serde_json::json!(1)),
            },
            extra: Default::default(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(
            !json.contains("claim-must-not-appear"),
            "Serialize must not leak claim tokens: {json}"
        );
        assert!(json.contains("[REDACTED]"));
        // Intentional persistence path:
        assert_eq!(
            resp.token.as_ref().map(ClaimToken::as_str),
            Some("claim-must-not-appear")
        );
    }
}
