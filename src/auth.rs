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
/// - [`Debug`] / [`Display`] redact the secret
/// - [`Serialize`] emits `"[REDACTED]"` so accidental `serde_json::to_string` logging
///   does not leak the token
/// - [`Deserialize`] accepts the real wire value from the Merchant Backend
/// - Memory is zeroized on drop
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ClaimToken(String);

impl ClaimToken {
    /// Wrap a claim token string from the API.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the raw claim token (use only when constructing pay URIs).
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
}
