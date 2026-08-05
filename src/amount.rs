//! Taler [`Amount`](https://docs.taler.net/core/api-common.html#amounts)
//! — fixed-precision currency strings, never binary floating point.
//!
//! Wire format: `CURRENCY:VALUE` where:
//! - `CURRENCY` is 1–11 ASCII letters (`a-zA-Z`)
//! - integer part of `VALUE` is ≤ 2⁵²
//! - fractional part has at most 8 digits
//! - trailing/leading dots are rejected (`EUR:1.`, `EUR:.1`)

use crate::error::MerchantError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Maximum integer units allowed in a Taler amount (`2^52`).
pub const MAX_AMOUNT_VALUE: u64 = 1u64 << 52;

/// Maximum length of a currency code.
pub const MAX_CURRENCY_LEN: usize = 11;

/// Maximum fractional decimal digits.
pub const MAX_FRACTIONAL_DIGITS: usize = 8;

/// A monetary amount in Taler wire format.
///
/// Values are stored as validated strings so arithmetic never goes through
/// `f64` / `f32`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Amount {
    currency: String,
    /// Canonical value text without a leading `+`/`-` (unsigned amount).
    value: String,
}

impl Amount {
    /// Parse and validate a Taler amount string per the common API grammar.
    ///
    /// # Errors
    ///
    /// Returns [`MerchantError::InvalidAmount`] if the string is malformed.
    pub fn parse(s: &str) -> Result<Self, MerchantError> {
        let original = s;
        let s = s.trim();
        if s.is_empty() {
            return Err(MerchantError::InvalidAmount(original.to_string()));
        }

        let (currency, value) = s
            .split_once(':')
            .ok_or_else(|| MerchantError::InvalidAmount(original.to_string()))?;

        if !is_valid_currency(currency) {
            return Err(MerchantError::InvalidAmount(original.to_string()));
        }
        if !is_valid_unsigned_decimal(value) {
            return Err(MerchantError::InvalidAmount(original.to_string()));
        }

        Ok(Self {
            currency: currency.to_string(),
            value: value.to_string(),
        })
    }

    /// Currency code (e.g. `KUDOS` or `EUR`).
    pub fn currency(&self) -> &str {
        &self.currency
    }

    /// Decimal value portion (e.g. `10` or `1.50`).
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Canonical wire form `CURRENCY:value`.
    pub fn as_str(&self) -> String {
        format!("{}:{}", self.currency, self.value)
    }
}

impl FromStr for Amount {
    type Err = MerchantError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.currency, self.value)
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Amount({self})")
    }
}

impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Amount::parse(&s).map_err(serde::de::Error::custom)
    }
}

fn is_valid_currency(currency: &str) -> bool {
    let len = currency.len();
    (1..=MAX_CURRENCY_LEN).contains(&len) && currency.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_valid_unsigned_decimal(value: &str) -> bool {
    if value.is_empty() || value.starts_with('+') || value.starts_with('-') {
        return false;
    }
    // Reject leading / trailing dots: "EUR:.1", "EUR:1."
    if value.starts_with('.') || value.ends_with('.') {
        return false;
    }

    let (int_part, frac_part) = match value.split_once('.') {
        None => (value, None),
        Some((i, f)) => (i, Some(f)),
    };

    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Disallow empty fractional side already handled; also reject multiple dots
    // via split_once + ensuring frac has no '.'.
    if let Some(frac) = frac_part {
        if frac.is_empty()
            || frac.len() > MAX_FRACTIONAL_DIGITS
            || !frac.bytes().all(|b| b.is_ascii_digit())
            || frac.contains('.')
        {
            return false;
        }
    }

    // Integer magnitude ≤ 2^52. Leading zeros are fine ("0001").
    match int_part.parse::<u64>() {
        Ok(n) => n <= MAX_AMOUNT_VALUE,
        Err(_) => false, // overflow beyond u64, certainly > 2^52
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_examples() {
        assert_eq!(Amount::parse("EUR:1.50").unwrap().to_string(), "EUR:1.50");
        assert_eq!(Amount::parse("EUR:10").unwrap().to_string(), "EUR:10");
        assert_eq!(Amount::parse("KUDOS:1").unwrap().currency(), "KUDOS");
        // Spec allows a-zA-Z (case-preserving).
        assert_eq!(Amount::parse("eur:10").unwrap().currency(), "eur");
    }

    #[test]
    fn accepts_boundary_integer_and_fraction() {
        let max = format!("EUR:{MAX_AMOUNT_VALUE}");
        assert!(Amount::parse(&max).is_ok());
        assert!(Amount::parse("EUR:0").is_ok());
        assert!(Amount::parse("EUR:1.12345678").is_ok()); // 8 frac digits
        assert!(Amount::parse("ABCDEFGHIJK:1").is_ok()); // 11 letter currency
    }

    #[test]
    fn rejects_spec_invalid_examples() {
        // From docs.taler.net/core/api-common.html#amounts
        assert!(Amount::parse("A:B:1.5").is_err());
        assert!(Amount::parse("EUR:4503599627370501.0").is_err()); // > 2^52
        assert!(Amount::parse("EUR:1.").is_err());
        assert!(Amount::parse("EUR:.1").is_err());
    }

    #[test]
    fn rejects_currency_and_fraction_rules() {
        assert!(Amount::parse("").is_err());
        assert!(Amount::parse("10").is_err());
        assert!(Amount::parse(":10").is_err());
        assert!(Amount::parse("EUR:").is_err());
        assert!(Amount::parse("EU1:10").is_err()); // digit in currency
        assert!(Amount::parse("ABCDEFGHIJKL:1").is_err()); // 12 chars
        assert!(Amount::parse("EUR:1.123456789").is_err()); // 9 frac
        assert!(Amount::parse("EUR:1.2.3").is_err());
        assert!(Amount::parse("EUR:abc").is_err());
        assert!(Amount::parse("+EUR:1").is_err());
        assert!(Amount::parse("EUR:+1").is_err());
        assert!(Amount::parse("EUR:-1").is_err());
        let over = format!("EUR:{}", MAX_AMOUNT_VALUE + 1);
        assert!(Amount::parse(&over).is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let a = Amount::parse("KUDOS:3.14").unwrap();
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, "\"KUDOS:3.14\"");
        let back: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
