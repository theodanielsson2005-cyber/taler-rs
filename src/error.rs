//! Typed errors for the merchant client.

use std::fmt;

/// Errors produced by [`crate::MerchantClient`] and related helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerchantError {
    /// Amount string failed validation.
    InvalidAmount(String),
    /// Caller supplied a guessable `order_id` while `create_token` is false.
    ///
    /// With `create_token: false`, Taler relies on order-id entropy instead of a
    /// claim token. Short or enumerable IDs create a scrapeable payment surface.
    WeakOrderId {
        /// The rejected order id (safe to log — not a secret).
        order_id: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// HTTP transport failure (DNS, TLS, connection reset, …).
    Transport(String),
    /// HTTP 401 / 403 from the merchant backend.
    Unauthorized {
        /// HTTP status code.
        status: u16,
        /// Optional Taler error hint from the body.
        hint: Option<String>,
    },
    /// HTTP 404 — instance or order unknown.
    NotFound {
        /// HTTP status code.
        status: u16,
        /// Optional Taler error hint from the body.
        hint: Option<String>,
    },
    /// HTTP 409 — order id already exists (or similar conflict).
    Conflict {
        /// HTTP status code.
        status: u16,
        /// Optional Taler error hint from the body.
        hint: Option<String>,
    },
    /// Other non-success HTTP response from the backend.
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body (truncated); never contains injected client secrets.
        body: String,
        /// Optional Taler `hint` field if the body was JSON.
        hint: Option<String>,
        /// Optional Taler numeric `code` if present.
        code: Option<i32>,
    },
    /// Response JSON did not match expected shapes.
    Protocol(String),
    /// Order was created, but the follow-up status GET failed.
    ///
    /// The backend already has `order_id`. Callers must not blindly retry
    /// `create_order` with a new id (duplicate offers). Fetch status or reuse
    /// the same id intentionally.
    CreatedButStatusFailed {
        /// Order id assigned by the successful POST.
        order_id: String,
        /// Display form of the status-fetch error.
        cause: String,
    },
    /// `create_order` expected unpaid + non-empty `taler_pay_uri`.
    UnexpectedOrderStatus {
        /// Order id from the successful POST.
        order_id: String,
        /// Status discriminator received (`claimed`, `paid`, …).
        got: String,
        /// Human-readable detail.
        detail: String,
    },
    /// Caller misconfigured the client (empty URL/token, etc.).
    Config(String),
}

impl fmt::Display for MerchantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MerchantError::InvalidAmount(s) => write!(f, "invalid Taler amount: {s:?}"),
            MerchantError::WeakOrderId { order_id, reason } => {
                write!(
                    f,
                    "weak order_id {order_id:?} with create_token=false: {reason}"
                )
            }
            MerchantError::Transport(msg) => write!(f, "transport error: {msg}"),
            MerchantError::Unauthorized { status, hint } => {
                write!(f, "unauthorized (HTTP {status})")?;
                if let Some(h) = hint {
                    write!(f, ": {h}")?;
                }
                Ok(())
            }
            MerchantError::NotFound { status, hint } => {
                write!(f, "not found (HTTP {status})")?;
                if let Some(h) = hint {
                    write!(f, ": {h}")?;
                }
                Ok(())
            }
            MerchantError::Conflict { status, hint } => {
                write!(f, "conflict (HTTP {status})")?;
                if let Some(h) = hint {
                    write!(f, ": {h}")?;
                }
                Ok(())
            }
            MerchantError::Http {
                status,
                hint,
                code,
                body,
            } => {
                write!(f, "merchant backend HTTP {status}")?;
                if let Some(c) = code {
                    write!(f, " (code {c})")?;
                }
                if let Some(h) = hint {
                    write!(f, ": {h}")?;
                } else if !body.is_empty() {
                    write!(f, ": {body}")?;
                }
                Ok(())
            }
            MerchantError::Protocol(msg) => write!(f, "protocol error: {msg}"),
            MerchantError::CreatedButStatusFailed { order_id, cause } => write!(
                f,
                "order {order_id} was created but status fetch failed: {cause}"
            ),
            MerchantError::UnexpectedOrderStatus {
                order_id,
                got,
                detail,
            } => write!(
                f,
                "order {order_id} has unexpected status {got:?}: {detail}"
            ),
            MerchantError::Config(msg) => write!(f, "configuration error: {msg}"),
        }
    }
}

impl std::error::Error for MerchantError {}

impl MerchantError {
    pub(crate) fn from_http(status: u16, body: &str) -> Self {
        let (hint, code) = parse_taler_error_fields(body);
        match status {
            401 | 403 => MerchantError::Unauthorized { status, hint },
            404 => MerchantError::NotFound { status, hint },
            409 => MerchantError::Conflict { status, hint },
            _ => MerchantError::Http {
                status,
                body: truncate(body, 512),
                hint,
                code,
            },
        }
    }
}

fn parse_taler_error_fields(body: &str) -> (Option<String>, Option<i32>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return (None, None);
    };
    let hint = v.get("hint").and_then(|x| x.as_str()).map(str::to_string);
    let code = v.get("code").and_then(|x| x.as_i64()).map(|n| n as i32);
    (hint, code)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
