//! HTTP client for the GNU Taler Merchant Backend (sync PoC).

use crate::auth::SecretToken;
use crate::error::MerchantError;
use crate::order_id::generate_order_id;
use crate::types::{
    CreateOrderRequest, CreateOrderResponse, MerchantOrderStatus, MerchantVersionResponse,
    OrderStatus, PostOrderResponse, StatusQuery,
};
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for [`MerchantClient`].
#[derive(Debug, Clone)]
pub struct MerchantConfig {
    /// Base URL including instance path when used, e.g.
    /// `https://backend.demo.taler.net/instances/sandbox/`.
    pub base_url: String,
    /// Bearer token material (with or without `secret-token:` prefix).
    pub token: SecretToken,
    /// HTTP timeout for each request.
    pub timeout: Duration,
}

impl MerchantConfig {
    /// Build config from base URL and token string.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: SecretToken::new(token),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the HTTP timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), MerchantError> {
        if self.base_url.trim().is_empty() {
            return Err(MerchantError::Config("base_url must not be empty".into()));
        }
        if self.token.is_empty() {
            return Err(MerchantError::Config("token must not be empty".into()));
        }
        Ok(())
    }
}

/// HTTP client for a Taler Merchant Backend instance.
#[derive(Debug, Clone)]
pub struct MerchantClient {
    config: MerchantConfig,
    agent: ureq::Agent,
}

impl MerchantClient {
    /// Create a client from [`MerchantConfig`].
    pub fn new(config: MerchantConfig) -> Result<Self, MerchantError> {
        config.validate()?;
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(config.timeout)
            .timeout_read(config.timeout)
            .build();
        Ok(Self { config, agent })
    }

    /// Convenience: URL + token.
    pub fn with_credentials(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, MerchantError> {
        Self::new(MerchantConfig::new(base_url, token))
    }

    /// Borrow the active configuration (token remains redacted in debug).
    pub fn config(&self) -> &MerchantConfig {
        &self.config
    }

    fn url(&self, path: &str) -> String {
        join_url(&self.config.base_url, path)
    }

    fn auth_header(&self) -> String {
        self.config.token.authorization_header_value()
    }

    /// `GET /config` — protocol version and default currency.
    pub fn get_config(&self) -> Result<MerchantVersionResponse, MerchantError> {
        let url = self.url("config");
        let resp = self
            .agent
            .get(&url)
            .set("Accept", "application/json")
            .call()
            .map_err(map_transport)?;
        read_json(resp)
    }

    /// `POST /private/orders` only (no follow-up status fetch).
    ///
    /// When `create_token` is `false`, the chosen `order_id` must pass
    /// [`crate::validate_unguessable_order_id`]. Auto-generated ids always do.
    pub fn post_order(
        &self,
        request: CreateOrderRequest,
    ) -> Result<PostOrderResponse, MerchantError> {
        let create_token = request.create_token;
        let order_id = match request.order_id.clone() {
            Some(id) => id,
            None => generate_order_id()?,
        };
        if !create_token {
            crate::order_id::validate_unguessable_order_id(&order_id)?;
        }
        if order_id.trim().is_empty() {
            return Err(MerchantError::Config("order_id must not be empty".into()));
        }

        let body = request.into_post(order_id);
        let url = self.url("private/orders");

        let resp = self
            .agent
            .post(&url)
            .set("Authorization", &self.auth_header())
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(map_transport)?;

        read_json(resp)
    }

    /// Create an order and fetch status so callers get a pay URI.
    ///
    /// This performs **two** HTTP calls: `POST /private/orders` then
    /// `GET /private/orders/{id}`.
    ///
    /// # Success guarantees
    ///
    /// On `Ok`, status is **unpaid** with a non-empty `taler_pay_uri` (see
    /// [`CreateOrderResponse::taler_pay_uri`]).
    ///
    /// # Failure modes
    ///
    /// - [`MerchantError::CreatedButStatusFailed`]: POST succeeded; status GET
    ///   failed. The `order_id` already exists — do **not** retry with a new id
    ///   blindly; call [`Self::get_order_status`] instead.
    /// - [`MerchantError::UnexpectedOrderStatus`]: status was not unpaid with a
    ///   pay URI (e.g. already claimed/paid).
    ///
    /// Use [`Self::post_order`] if you only want the create response.
    pub fn create_order(
        &self,
        request: CreateOrderRequest,
    ) -> Result<CreateOrderResponse, MerchantError> {
        let created = self.post_order(request)?;
        let status = match self.get_order_status_raw(&created.order_id, &StatusQuery::default()) {
            Ok(status) => status,
            Err(cause) => {
                return Err(MerchantError::CreatedButStatusFailed {
                    order_id: created.order_id,
                    cause: cause.to_string(),
                });
            }
        };

        match &status {
            MerchantOrderStatus::Unpaid(u)
                if !u.taler_pay_uri.trim().is_empty() && !u.order_status_url.trim().is_empty() =>
            {
                Ok(CreateOrderResponse {
                    order_id: created.order_id,
                    token: created.token,
                    status,
                })
            }
            other => Err(MerchantError::UnexpectedOrderStatus {
                order_id: created.order_id,
                got: other.as_str().to_string(),
                detail: "create_order requires unpaid status with non-empty taler_pay_uri and order_status_url"
                    .into(),
            }),
        }
    }

    /// `GET /private/orders/{order_id}` — typed status union.
    pub fn get_order_status_raw(
        &self,
        order_id: &str,
        query: &StatusQuery,
    ) -> Result<MerchantOrderStatus, MerchantError> {
        if order_id.trim().is_empty() {
            return Err(MerchantError::Config("order_id must not be empty".into()));
        }

        let encoded_id = percent_encode_path_segment(order_id);
        let mut url = self.url(&format!("private/orders/{encoded_id}"));
        let mut q = Vec::new();
        if let Some(ms) = query.timeout_ms {
            q.push(format!("timeout_ms={ms}"));
        }
        if let Some(sid) = &query.session_id {
            if !sid.is_empty() {
                q.push(format!("session_id={}", percent_encode_path_segment(sid)));
            }
        }
        if !q.is_empty() {
            url.push('?');
            url.push_str(&q.join("&"));
        }

        // Long-poll may exceed the default read timeout; bump when requested.
        let agent = if let Some(ms) = query.timeout_ms {
            let read = self.config.timeout + Duration::from_millis(ms.saturating_add(1_000));
            ureq::AgentBuilder::new()
                .timeout_connect(self.config.timeout)
                .timeout_read(read)
                .build()
        } else {
            self.agent.clone()
        };

        let resp = agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .set("Accept", "application/json")
            .call()
            .map_err(map_transport)?;

        read_json(resp)
    }

    /// `GET /private/orders/{order_id}` wrapped with the requested id.
    pub fn get_order_status(
        &self,
        order_id: &str,
        query: &StatusQuery,
    ) -> Result<OrderStatus, MerchantError> {
        let status = self.get_order_status_raw(order_id, query)?;
        Ok(OrderStatus {
            order_id: order_id.to_string(),
            status,
        })
    }
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

fn percent_encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn map_transport(err: ureq::Error) -> MerchantError {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            MerchantError::from_http(code, &body)
        }
        other => MerchantError::Transport(other.to_string()),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(resp: ureq::Response) -> Result<T, MerchantError> {
    let status = resp.status();
    let body = resp
        .into_string()
        .map_err(|e| MerchantError::Transport(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(MerchantError::from_http(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| {
        MerchantError::Protocol(format!(
            "failed to decode JSON response: {e}; body={}",
            truncate_for_error(&body, 256)
        ))
    })
}

fn truncate_for_error(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_strips_slashes() {
        assert_eq!(
            join_url("https://example.com/instances/sandbox/", "/private/orders"),
            "https://example.com/instances/sandbox/private/orders"
        );
        assert_eq!(
            join_url("https://example.com/instances/sandbox", "config"),
            "https://example.com/instances/sandbox/config"
        );
    }

    #[test]
    fn percent_encodes_unsafe_order_ids() {
        assert_eq!(percent_encode_path_segment("o-abc"), "o-abc");
        assert_eq!(percent_encode_path_segment("a/b"), "a%2Fb");
        assert_eq!(percent_encode_path_segment("a b"), "a%20b");
    }

    #[test]
    fn rejects_empty_config() {
        let err = MerchantClient::with_credentials("", "sandbox").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)));
        let err = MerchantClient::with_credentials("https://x/", "").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)));
    }
}
