//! HTTP client for the GNU Taler Merchant Backend (sync PoC).

use crate::auth::SecretToken;
use crate::error::MerchantError;
use crate::order_id::generate_order_id;
use crate::types::{
    CreateOrderRequest, CreateOrderResponse, MerchantOrderStatus, MerchantVersionResponse,
    OrderStatus, PostOrderResponse, StatusQuery,
};
use std::time::Duration;
use url::Url;
use zeroize::Zeroize;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for [`MerchantClient`].
#[derive(Debug, Clone)]
pub struct MerchantConfig {
    /// Base URL including instance path when used, e.g.
    /// `https://backend.demo.taler.net/instances/sandbox/`.
    ///
    /// Must be `https://`, or `http://` only for loopback hosts (`127.0.0.1`,
    /// `localhost`, `::1`) used by local tests.
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
        validate_base_url(&self.base_url)?;
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
        let agent = build_agent(config.timeout, config.timeout);
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
        if request.summary.trim().is_empty() {
            return Err(MerchantError::Config("summary must not be empty".into()));
        }
        validate_fulfillment_url(&request.fulfillment_url)?;

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
        let auth = ZeroizingHeader(self.config.token.authorization_header_value());

        self.agent
            .post(&url)
            .set("Authorization", auth.as_str())
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(map_transport)
            .and_then(read_json)
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
                    cause: Box::new(cause),
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
                q.push(format!(
                    "session_id={}",
                    percent_encode_query_component(sid)
                ));
            }
        }
        if !q.is_empty() {
            url.push('?');
            url.push_str(&q.join("&"));
        }

        // Long-poll may exceed the default read timeout; bump when requested.
        let agent = if let Some(ms) = query.timeout_ms {
            let read = self.config.timeout + Duration::from_millis(ms.saturating_add(1_000));
            build_agent(self.config.timeout, read)
        } else {
            self.agent.clone()
        };

        let auth = ZeroizingHeader(self.config.token.authorization_header_value());
        agent
            .get(&url)
            .set("Authorization", auth.as_str())
            .set("Accept", "application/json")
            .call()
            .map_err(map_transport)
            .and_then(read_json)
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

fn build_agent(connect: Duration, read: Duration) -> ureq::Agent {
    // redirects(0): never follow 3xx. Even with RedirectAuthHeaders::Never (ureq
    // default), following redirects can change host/path unexpectedly; we refuse.
    ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout_read(read)
        .redirects(0)
        .redirect_auth_headers(ureq::RedirectAuthHeaders::Never)
        .build()
}

/// Bearer header buffer that is always zeroized on drop (including panic unwind).
struct ZeroizingHeader(String);

impl ZeroizingHeader {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for ZeroizingHeader {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn validate_base_url(raw: &str) -> Result<(), MerchantError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(MerchantError::Config("base_url must not be empty".into()));
    }
    let parsed = Url::parse(raw)
        .map_err(|e| MerchantError::Config(format!("base_url is not a valid URL: {e}")))?;
    if parsed.cannot_be_a_base() || parsed.host_str().filter(|h| !h.is_empty()).is_none() {
        return Err(MerchantError::Config(
            "base_url must include a host (e.g. https://backend.example/instances/sandbox/)".into(),
        ));
    }
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => match parsed.host_str().filter(|h| !h.is_empty()) {
            Some("127.0.0.1") | Some("localhost") | Some("::1") => Ok(()),
            Some(host) => Err(MerchantError::Config(format!(
                "http:// base_url only allowed for loopback (127.0.0.1/localhost/::1), got host {host:?}"
            ))),
            None => Err(MerchantError::Config(
                "http:// base_url requires a loopback host".into(),
            )),
        },
        other => Err(MerchantError::Config(format!(
            "base_url scheme must be https (or http for loopback tests), got {other:?}"
        ))),
    }
}

/// Fulfillment URL shown after payment — must be an absolute http(s) URL.
fn validate_fulfillment_url(raw: &str) -> Result<(), MerchantError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(MerchantError::Config(
            "fulfillment_url must not be empty".into(),
        ));
    }
    let parsed = Url::parse(raw)
        .map_err(|e| MerchantError::Config(format!("fulfillment_url is not a valid URL: {e}")))?;
    if parsed.cannot_be_a_base() || parsed.host_str().filter(|h| !h.is_empty()).is_none() {
        return Err(MerchantError::Config(
            "fulfillment_url must be an absolute URL with a host".into(),
        ));
    }
    match parsed.scheme() {
        "https" | "http" => Ok(()),
        other => Err(MerchantError::Config(format!(
            "fulfillment_url scheme must be http or https, got {other:?}"
        ))),
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

/// application/x-www-form-urlencoded component (query values).
fn percent_encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn map_transport(err: ureq::Error) -> MerchantError {
    match err {
        ureq::Error::Status(code, resp) if (300..400).contains(&code) => {
            let location = resp.header("location").map(str::to_string);
            // Drain body so the connection can be reused; ignore content.
            let _ = resp.into_string();
            MerchantError::RedirectDisallowed {
                status: code,
                location,
            }
        }
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            MerchantError::from_http(code, &body)
        }
        other => MerchantError::Transport(other.to_string()),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(resp: ureq::Response) -> Result<T, MerchantError> {
    let status = resp.status();
    if (300..400).contains(&status) {
        let location = resp.header("location").map(str::to_string);
        let _ = resp.into_string();
        return Err(MerchantError::RedirectDisallowed { status, location });
    }
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
    fn query_component_uses_plus_for_space() {
        assert_eq!(percent_encode_query_component("a b"), "a+b");
        assert_eq!(percent_encode_query_component("a/b"), "a%2Fb");
    }

    #[test]
    fn rejects_empty_config() {
        let err = MerchantClient::with_credentials("", "sandbox").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)));
        let err = MerchantClient::with_credentials("https://x/", "").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)));
    }

    #[test]
    fn rejects_non_https_non_loopback() {
        let err = MerchantClient::with_credentials("http://example.com/m/", "sandbox").unwrap_err();
        assert!(
            matches!(err, MerchantError::Config(ref m) if m.contains("loopback")),
            "{err}"
        );
        // Homograph-ish bypass attempt
        let err =
            MerchantClient::with_credentials("http://127.0.0.1.evil.com/m/", "t").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)));
    }

    #[test]
    fn accepts_https_and_loopback_http() {
        assert!(MerchantClient::with_credentials("https://backend.example/m/", "t").is_ok());
        assert!(MerchantClient::with_credentials("http://127.0.0.1:8080/m/", "t").is_ok());
        assert!(MerchantClient::with_credentials("http://localhost:9/m/", "t").is_ok());
    }

    #[test]
    fn rejects_https_without_host() {
        let err = MerchantClient::with_credentials("https://", "t").unwrap_err();
        assert!(matches!(err, MerchantError::Config(_)), "{err}");
        // `https:///nohost` is parsed by rust-url as host "nohost" (not a missing-host bypass).
        assert!(MerchantClient::with_credentials("https://nohost/", "t").is_ok());
    }

    #[test]
    fn fulfillment_url_rules() {
        assert!(validate_fulfillment_url("https://example.com/thanks").is_ok());
        assert!(validate_fulfillment_url("http://example.com/thanks").is_ok());
        assert!(validate_fulfillment_url("").is_err());
        assert!(validate_fulfillment_url("javascript:alert(1)").is_err());
        assert!(validate_fulfillment_url("/relative").is_err());
        assert!(validate_fulfillment_url("https://").is_err());
    }
}
