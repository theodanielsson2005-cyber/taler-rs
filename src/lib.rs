//! `taler-merchant` — Rust client for the GNU Taler Merchant Backend API.
//!
//! Pre-funding / NGI TALER **happy-path proof of concept**:
//! `GET /config`, `POST /private/orders`, `GET /private/orders/{id}`.
//!
//! # Privacy defaults
//!
//! - Prefer [`generate_order_id`] and `create_token: false` so order references
//!   are unguessable without claim tokens. Weak custom IDs are rejected when
//!   claim tokens are disabled.
//! - [`SecretToken`] and [`ClaimToken`] redact secrets in `Debug`/`Display`;
//!   [`ClaimToken`] also redacts on `Serialize`; both zeroize on drop.
//! - Do not put buyer PII in fulfillment URLs or logs; do not expose public
//!   order listings in adopter shops.
//!
//! # Protocol notes
//!
//! Types track the Merchant Backend REST shapes for the endpoints above.
//! [`Amount`] follows the official common-API grammar (no `f64`).
//! Status responses are a tagged union: unpaid / claimed / paid.
//! See `docs/API_COVERAGE.md` in the repository for the honest endpoint inventory.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod amount;
mod auth;
mod client;
mod error;
mod order_id;
mod types;

pub use amount::{Amount, MAX_AMOUNT_VALUE, MAX_CURRENCY_LEN, MAX_FRACTIONAL_DIGITS};
pub use auth::{ClaimToken, SecretToken};
pub use client::{MerchantClient, MerchantConfig};
pub use error::MerchantError;
pub use order_id::{
    generate_order_id, is_unguessable_order_id, validate_unguessable_order_id,
    MIN_UNGUESSABLE_ORDER_ID_LEN,
};
pub use types::{
    CheckPaymentClaimed, CheckPaymentPaid, CheckPaymentUnpaid, ContractTerms, CreateOrderRequest,
    CreateOrderResponse, MerchantOrderStatus, MerchantOrderStatusResponse, MerchantVersionResponse,
    OrderDraft, OrderStatus, PostOrderRequest, PostOrderResponse, ProtoContractTerms, StatusQuery,
    Timestamp,
};
