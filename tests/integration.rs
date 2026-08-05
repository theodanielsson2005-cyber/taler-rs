//! Offline fixture deserialization + optional live sandbox smoke test.

use taler_merchant::{
    Amount, CreateOrderRequest, MerchantClient, MerchantOrderStatus, MerchantVersionResponse,
    PostOrderRequest, PostOrderResponse, StatusQuery,
};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[test]
fn fixture_config_deserializes_required_and_extra() {
    let cfg: MerchantVersionResponse = serde_json::from_str(&fixture("config.json")).unwrap();
    assert_eq!(cfg.name, "taler-merchant");
    assert_eq!(cfg.currency, "KUDOS");
    assert_eq!(cfg.version, "30:0:18");
    assert!(cfg.extra.contains_key("currencies"));
    assert!(cfg.extra.contains_key("exchanges"));
}

#[test]
fn fixture_post_order_deserializes() {
    let resp: PostOrderResponse =
        serde_json::from_str(&fixture("post_order_response.json")).unwrap();
    assert_eq!(resp.order_id, "o-0123456789abcdef0123456789abcdef");
    assert!(resp.token.is_none());
    assert!(resp.pay_deadline.t_s.is_some());
}

#[test]
fn fixture_unpaid_status_is_typed_union() {
    let status: MerchantOrderStatus =
        serde_json::from_str(&fixture("order_status_unpaid.json")).unwrap();
    match status {
        MerchantOrderStatus::Unpaid(u) => {
            assert!(u.taler_pay_uri.starts_with("taler://pay/"));
            assert_eq!(u.summary.as_deref(), Some("Donation"));
            assert_eq!(
                u.total_amount.as_ref().map(ToString::to_string).as_deref(),
                Some("KUDOS:1")
            );
            assert!(u.proto_contract_terms.is_some());
        }
        other => panic!("expected unpaid, got {other:?}"),
    }
}

#[test]
fn fixture_claimed_status_is_typed_union() {
    let status: MerchantOrderStatus =
        serde_json::from_str(&fixture("order_status_claimed.json")).unwrap();
    match status {
        MerchantOrderStatus::Claimed(c) => {
            assert_eq!(c.contract_terms.summary, "Donation");
            assert!(!c.order_status_url.is_empty());
        }
        other => panic!("expected claimed, got {other:?}"),
    }
}

#[test]
fn fixture_paid_status_exposes_deposit_total() {
    let status: MerchantOrderStatus =
        serde_json::from_str(&fixture("order_status_paid.json")).unwrap();
    assert!(status.is_paid());
    match status {
        MerchantOrderStatus::Paid(p) => {
            assert_eq!(p.deposit_total.to_string(), "KUDOS:1");
            assert!(!p.refunded);
            assert_eq!(p.contract_terms.summary, "Donation");
        }
        other => panic!("expected paid, got {other:?}"),
    }
}

#[test]
fn create_order_request_serializes_minimal_body() {
    let amount = Amount::parse("KUDOS:10").unwrap();
    let req = CreateOrderRequest::new("Donation", amount, "https://example.com/thanks")
        .with_order_id("o-fixed-for-test")
        .with_create_token(false);
    let post = PostOrderRequest {
        order: taler_merchant::OrderDraft {
            amount: req.amount.clone(),
            summary: req.summary.clone(),
            fulfillment_url: req.fulfillment_url.clone(),
            order_id: req.order_id.clone(),
        },
        create_token: Some(req.create_token),
    };
    let body = serde_json::to_value(post).unwrap();
    assert_eq!(body["order"]["amount"], "KUDOS:10");
    assert_eq!(body["order"]["summary"], "Donation");
    assert_eq!(body["order"]["order_id"], "o-fixed-for-test");
    assert_eq!(body["create_token"], false);
}

/// Live smoke test against the public demo sandbox.
///
/// ```text
/// cargo test --test integration -- --ignored --nocapture
/// ```
#[test]
#[ignore = "hits https://backend.demo.taler.net — run with --ignored when online"]
fn live_sandbox_config_create_status() {
    let client = MerchantClient::with_credentials(
        "https://backend.demo.taler.net/instances/sandbox/",
        "sandbox",
    )
    .expect("client");

    let cfg = client.get_config().expect("config");
    assert_eq!(cfg.name, "taler-merchant");
    assert_eq!(cfg.currency, "KUDOS");
    assert!(cfg.extra.contains_key("currencies") || !cfg.version.is_empty());

    let created = client
        .create_order(CreateOrderRequest::new(
            "taler-rs PoC live test",
            Amount::parse("KUDOS:1").unwrap(),
            "https://example.com/thanks",
        ))
        .expect("create_order");

    assert!(!created.order_id.is_empty());
    assert_eq!(created.order_status_str(), "unpaid");
    assert!(
        created.taler_pay_uri().starts_with("taler://"),
        "expected taler_pay_uri, got {}",
        created.taler_pay_uri()
    );

    let status = client
        .get_order_status(&created.order_id, &StatusQuery::default())
        .expect("status");
    assert_eq!(status.order_status(), "unpaid");
}
