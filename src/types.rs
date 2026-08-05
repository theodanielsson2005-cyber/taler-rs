//! Request/response types for the Merchant Backend REST API (PoC surface).
//!
//! Status responses follow the official tagged union on `order_status`
//! (`unpaid` | `claimed` | `paid`). Unknown future fields are preserved via
//! `#[serde(flatten)]` where practical so protocol evolution does not break
//! deserialization of required fields.

use crate::amount::Amount;
use crate::auth::ClaimToken;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration returned by `GET /config`.
///
/// Required fields are typed; additional backend fields are retained in
/// [`MerchantVersionResponse::extra`] for forward compatibility.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct MerchantVersionResponse {
    /// Libtool-style protocol version (`current:revision:age`).
    pub version: String,
    /// Protocol name; expected `"taler-merchant"`.
    pub name: String,
    /// Default currency suggested by the backend.
    pub currency: String,
    /// Optional implementation URN.
    #[serde(default)]
    pub implementation: Option<String>,
    /// Optional source build version (`@since` protocol v33).
    #[serde(default)]
    pub build_version: Option<String>,
    /// Additional `/config` fields (currencies, exchanges, delays, …).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Body for `POST /private/orders`.
#[derive(Debug, Clone, Serialize)]
pub struct PostOrderRequest {
    /// Minimal order / contract terms fields.
    pub order: OrderDraft,
    /// When `false` and `order_id` is high-entropy, claim tokens are unnecessary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_token: Option<bool>,
}

/// Fields required to create a simple order (PoC).
#[derive(Debug, Clone, Serialize)]
pub struct OrderDraft {
    /// Amount to charge.
    pub amount: Amount,
    /// Short human-readable summary.
    pub summary: String,
    /// URL shown / opened after successful payment.
    pub fulfillment_url: String,
    /// Optional caller-chosen order id (prefer [`crate::generate_order_id`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
}

/// Convenience builder used by the library API and CLI.
#[derive(Debug, Clone)]
pub struct CreateOrderRequest {
    /// Short human-readable summary.
    pub summary: String,
    /// Amount to charge.
    pub amount: Amount,
    /// Fulfillment URL after payment.
    pub fulfillment_url: String,
    /// Optional order id; if `None`, an unguessable id is generated.
    pub order_id: Option<String>,
    /// Whether the backend should create a claim token (default `false` for PoC).
    pub create_token: bool,
}

impl CreateOrderRequest {
    /// Build a minimal create-order request.
    pub fn new(
        summary: impl Into<String>,
        amount: Amount,
        fulfillment_url: impl Into<String>,
    ) -> Self {
        Self {
            summary: summary.into(),
            amount,
            fulfillment_url: fulfillment_url.into(),
            order_id: None,
            create_token: false,
        }
    }

    /// Set an explicit order id (should be unguessable if `create_token` is false).
    pub fn with_order_id(mut self, order_id: impl Into<String>) -> Self {
        self.order_id = Some(order_id.into());
        self
    }

    /// Request a claim token from the backend (defaults to off in this PoC).
    pub fn with_create_token(mut self, create_token: bool) -> Self {
        self.create_token = create_token;
        self
    }

    pub(crate) fn into_post(self, order_id: String) -> PostOrderRequest {
        PostOrderRequest {
            order: OrderDraft {
                amount: self.amount,
                summary: self.summary,
                fulfillment_url: self.fulfillment_url,
                order_id: Some(order_id),
            },
            create_token: Some(self.create_token),
        }
    }
}

/// Response from `POST /private/orders`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PostOrderResponse {
    /// Order identifier.
    pub order_id: String,
    /// Optional claim token when `create_token` was true.
    #[serde(default)]
    pub token: Option<ClaimToken>,
    /// Payment deadline (`@since` v21). Required by current Merchant protocol.
    pub pay_deadline: Timestamp,
    /// Forward-compatible extra fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// High-level create result including pay URI when status was fetched.
#[derive(Debug, Clone)]
pub struct CreateOrderResponse {
    /// Order identifier.
    pub order_id: String,
    /// Claim token if the backend issued one.
    pub token: Option<ClaimToken>,
    /// Full typed status after the follow-up GET (expected: unpaid).
    pub status: MerchantOrderStatus,
}

impl CreateOrderResponse {
    /// `taler://pay/…` — always present after a successful [`crate::MerchantClient::create_order`].
    pub fn taler_pay_uri(&self) -> &str {
        self.status
            .taler_pay_uri()
            .expect("create_order guarantees unpaid status with taler_pay_uri")
    }

    /// Browser status URL when provided by the unpaid status payload.
    pub fn order_status_url(&self) -> &str {
        self.status
            .order_status_url()
            .expect("create_order guarantees unpaid status with order_status_url")
    }

    /// Discriminator string: always `"unpaid"` after successful `create_order`.
    pub fn order_status_str(&self) -> &'static str {
        self.status.as_str()
    }
}

/// Optional long-poll parameters for status queries.
#[derive(Debug, Clone, Default)]
pub struct StatusQuery {
    /// Long-poll timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Optional session binding.
    pub session_id: Option<String>,
}

/// Absolute timestamp as used across Taler JSON APIs.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Timestamp {
    /// Seconds since Unix epoch, or the string `"never"` in some contexts.
    #[serde(default)]
    pub t_s: Option<Value>,
}

/// Contract terms subset used by order status responses.
///
/// Required commercial fields are typed; the remainder of the contract is
/// preserved in [`ContractTerms::extra`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ContractTerms {
    /// Human-readable summary.
    pub summary: String,
    /// Total amount for the contract (v0 orders).
    #[serde(default)]
    pub amount: Option<Amount>,
    /// Additional contract-term fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Proto contract terms on unpaid orders (`@since` v25) — same shape as
/// contract terms without a wallet nonce.
pub type ProtoContractTerms = ContractTerms;

/// Private order status: official tagged union on `order_status`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "order_status")]
pub enum MerchantOrderStatus {
    /// Not yet claimed by a wallet.
    #[serde(rename = "unpaid")]
    Unpaid(CheckPaymentUnpaid),
    /// Claimed by a wallet, payment not completed.
    #[serde(rename = "claimed")]
    Claimed(CheckPaymentClaimed),
    /// Fully paid.
    #[serde(rename = "paid")]
    Paid(CheckPaymentPaid),
}

impl MerchantOrderStatus {
    /// Wire discriminator.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unpaid(_) => "unpaid",
            Self::Claimed(_) => "claimed",
            Self::Paid(_) => "paid",
        }
    }

    /// Whether the order is paid.
    pub fn is_paid(&self) -> bool {
        matches!(self, Self::Paid(_))
    }

    /// Wallet pay URI (unpaid only).
    pub fn taler_pay_uri(&self) -> Option<&str> {
        match self {
            Self::Unpaid(u) => Some(u.taler_pay_uri.as_str()),
            _ => None,
        }
    }

    /// Status / QR page URL when present.
    pub fn order_status_url(&self) -> Option<&str> {
        match self {
            Self::Unpaid(u) => Some(u.order_status_url.as_str()),
            Self::Claimed(c) => Some(c.order_status_url.as_str()),
            Self::Paid(p) => Some(p.order_status_url.as_str()),
        }
    }

    /// Contract summary when available.
    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::Unpaid(u) => u
                .summary
                .as_deref()
                .or_else(|| u.proto_contract_terms.as_ref().map(|c| c.summary.as_str())),
            Self::Claimed(c) => Some(c.contract_terms.summary.as_str()),
            Self::Paid(p) => Some(p.contract_terms.summary.as_str()),
        }
    }
}

/// Unpaid private order status (`CheckPaymentUnpaidResponse`).
///
/// Required fields match the Merchant Backend private status contract used by
/// this PoC (protocol ~30 on the public demo). Deprecated-but-still-sent fields
/// (`summary`, `pay_deadline`, `total_amount`) remain optional. `proto_contract_terms`
/// is optional so slightly older backends still deserialize.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CheckPaymentUnpaid {
    /// URI that the wallet must process.
    pub taler_pay_uri: String,
    /// Browser status / QR page.
    pub order_status_url: String,
    /// Order creation time.
    pub creation_time: Timestamp,
    /// Pay deadline (deprecated in v25 in favor of proto_contract_terms).
    #[serde(default)]
    pub pay_deadline: Option<Timestamp>,
    /// Summary (deprecated in v25).
    #[serde(default)]
    pub summary: Option<String>,
    /// Proto contract terms (`@since` v25).
    #[serde(default)]
    pub proto_contract_terms: Option<ProtoContractTerms>,
    /// Total amount (deprecated in v25).
    #[serde(default)]
    pub total_amount: Option<Amount>,
    /// Already-paid order in the same session.
    #[serde(default)]
    pub already_paid_order_id: Option<String>,
    /// Fulfillment URL of an already-paid order in-session.
    #[serde(default)]
    pub already_paid_fulfillment_url: Option<String>,
    /// Forward-compatible fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Claimed but unpaid private order status.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CheckPaymentClaimed {
    /// Contract terms after claim.
    pub contract_terms: ContractTerms,
    /// Browser status / QR page.
    pub order_status_url: String,
    /// Forward-compatible fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Paid private order status (`CheckPaymentPaidResponse`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CheckPaymentPaid {
    /// Whether any refund was granted.
    pub refunded: bool,
    /// Approved refunds not yet obtained by the wallet.
    pub refund_pending: bool,
    /// Whether the exchange wired funds to the merchant.
    pub wired: bool,
    /// Total deposited (excluding fees).
    pub deposit_total: Amount,
    /// Exchange tracking error code (0 = none).
    pub exchange_code: i64,
    /// HTTP status from exchange tracking (0 = none).
    pub exchange_http_status: u16,
    /// Total refunded amount (zero if not refunded).
    pub refund_amount: Amount,
    /// Full contract terms.
    pub contract_terms: ContractTerms,
    /// Last payment timestamp (`@since` v14).
    pub last_payment: Timestamp,
    /// Wire transfer details.
    #[serde(default)]
    pub wire_details: Vec<Value>,
    /// Per-coin refund details.
    #[serde(default)]
    pub refund_details: Vec<Value>,
    /// External refund bookkeeping (`@since` vMixedPayments).
    #[serde(default)]
    pub refunds_external: Vec<Value>,
    /// Browser status URL.
    pub order_status_url: String,
    /// Selected choice index (`@since` v21).
    #[serde(default)]
    pub choice_index: Option<i32>,
    /// Forward-compatible fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Alias kept for earlier PoC naming in docs/tests.
pub type MerchantOrderStatusResponse = MerchantOrderStatus;

/// Normalized view used by simple callers (CLI). Prefer [`MerchantOrderStatus`].
#[derive(Debug, Clone, PartialEq)]
pub struct OrderStatus {
    /// Order identifier requested.
    pub order_id: String,
    /// Typed backend status.
    pub status: MerchantOrderStatus,
}

impl OrderStatus {
    /// Discriminator string.
    pub fn order_status(&self) -> &'static str {
        self.status.as_str()
    }

    /// Pay URI when unpaid.
    pub fn taler_pay_uri(&self) -> Option<&str> {
        self.status.taler_pay_uri()
    }

    /// Status URL when present.
    pub fn order_status_url(&self) -> Option<&str> {
        self.status.order_status_url()
    }

    /// Summary when available.
    pub fn summary(&self) -> Option<&str> {
        self.status.summary()
    }
}
