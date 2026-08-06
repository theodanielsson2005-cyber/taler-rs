//! HTTP-level contract tests with a mock Merchant Backend.
//!
//! These assert method, path, Authorization header, and JSON bodies — not
//! merely that fixtures deserialize.

use httpmock::prelude::*;
use serde_json::json;
use std::error::Error;
use taler_merchant::{
    Amount, CreateOrderRequest, MerchantClient, MerchantError, MerchantOrderStatus, StatusQuery,
};

fn unpaid_body(order_id: &str) -> serde_json::Value {
    json!({
        "order_status": "unpaid",
        "taler_pay_uri": format!("taler://pay/example.test/{order_id}/"),
        "order_status_url": format!("https://example.test/orders/{order_id}"),
        "summary": "Mock order",
        "total_amount": "KUDOS:1",
        "creation_time": { "t_s": 1700000000 },
        "pay_deadline": { "t_s": 1893456000 }
    })
}

fn claimed_body(order_id: &str) -> serde_json::Value {
    json!({
        "order_status": "claimed",
        "contract_terms": {
            "summary": "Claimed mock",
            "amount": "KUDOS:1",
            "order_id": order_id
        },
        "order_status_url": format!("https://example.test/orders/{order_id}")
    })
}

#[test]
fn get_config_hits_config_path() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET).path("/instances/sandbox/config");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "name": "taler-merchant",
                "version": "30:0:18",
                "currency": "KUDOS",
                "implementation": "urn:test"
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();
    let cfg = client.get_config().unwrap();
    mock.assert();
    assert_eq!(cfg.name, "taler-merchant");
    assert_eq!(cfg.currency, "KUDOS");
}

#[test]
fn post_order_sends_bearer_and_body() {
    let server = MockServer::start();
    let order_id = "o-deadbeefdeadbeefdeadbeefdeadbeef";

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/instances/sandbox/private/orders")
            .header("Authorization", "Bearer secret-token:sandbox")
            .header("content-type", "application/json")
            .json_body_partial(
                r#"{"create_token":false,"order":{"amount":"KUDOS:1","summary":"HTTP mock","fulfillment_url":"https://example.com/thanks","order_id":"o-deadbeefdeadbeefdeadbeefdeadbeef"}}"#,
            );
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": order_id,
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let resp = client
        .post_order(
            CreateOrderRequest::new(
                "HTTP mock",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap();

    mock.assert();
    assert_eq!(resp.order_id, order_id);
    assert!(resp.token.is_none());
}

#[test]
fn post_order_rejects_weak_id_without_claim_token() {
    let server = MockServer::start();
    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .post_order(
            CreateOrderRequest::new(
                "weak",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id("42"),
        )
        .unwrap_err();

    assert!(matches!(err, MerchantError::WeakOrderId { .. }));
}

#[test]
fn post_order_allows_weak_id_when_create_token_true() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/instances/sandbox/private/orders")
            .json_body_partial(r#"{"create_token":true,"order":{"order_id":"42"}}"#);
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": "42",
                "token": "claim-secret-value",
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let resp = client
        .post_order(
            CreateOrderRequest::new(
                "tok",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id("42")
            .with_create_token(true),
        )
        .unwrap();

    mock.assert();
    assert_eq!(resp.token.as_ref().unwrap().as_str(), "claim-secret-value");
    let leaked = serde_json::to_string(&resp.token).unwrap();
    assert!(!leaked.contains("claim-secret-value"));
}

#[test]
fn create_order_performs_post_then_status_get() {
    let server = MockServer::start();
    let order_id = "o-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let post = server.mock(|when, then| {
        when.method(POST).path("/instances/sandbox/private/orders");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": order_id,
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let status = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"))
            .header("Authorization", "Bearer secret-token:sandbox");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(unpaid_body(order_id));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "secret-token:sandbox",
    )
    .unwrap();

    let created = client
        .create_order(
            CreateOrderRequest::new(
                "seq",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap();

    post.assert();
    status.assert();
    assert_eq!(created.order_id, order_id);
    assert_eq!(created.order_status_str(), "unpaid");
    let expected_uri = format!("taler://pay/example.test/{order_id}/");
    assert_eq!(created.taler_pay_uri(), expected_uri);
}

#[test]
fn create_order_orphan_when_status_get_fails() {
    let server = MockServer::start();
    let order_id = "o-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let _post = server.mock(|when, then| {
        when.method(POST).path("/instances/sandbox/private/orders");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": order_id,
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let _status = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"));
        then.status(500)
            .header("content-type", "application/json")
            .json_body(json!({ "code": 1, "hint": "backend down" }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .create_order(
            CreateOrderRequest::new(
                "orphan",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap_err();

    assert!(Error::source(&err).is_some());
    match &err {
        MerchantError::CreatedButStatusFailed {
            order_id: oid,
            cause,
        } => {
            assert_eq!(oid, order_id);
            assert!(
                matches!(cause.as_ref(), MerchantError::Http { status: 500, .. })
                    || cause.to_string().contains("500")
                    || cause.to_string().contains("backend")
            );
        }
        other => panic!("expected CreatedButStatusFailed, got {other}"),
    }
}

#[test]
fn create_order_rejects_non_unpaid_status() {
    let server = MockServer::start();
    let order_id = "o-cccccccccccccccccccccccccccccccc";

    let _post = server.mock(|when, then| {
        when.method(POST).path("/instances/sandbox/private/orders");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": order_id,
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let _status = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(claimed_body(order_id));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .create_order(
            CreateOrderRequest::new(
                "claimed",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap_err();

    match err {
        MerchantError::UnexpectedOrderStatus {
            order_id: oid, got, ..
        } => {
            assert_eq!(oid, order_id);
            assert_eq!(got, "claimed");
        }
        other => panic!("expected UnexpectedOrderStatus, got {other}"),
    }
}

#[test]
fn get_order_status_maps_claimed() {
    let server = MockServer::start();
    let order_id = "o-claimedclaimedclaimedclaimedclaim";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(claimed_body(order_id));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let status = client
        .get_order_status(order_id, &StatusQuery::default())
        .unwrap();
    mock.assert();
    assert!(matches!(status.status, MerchantOrderStatus::Claimed(_)));
}

#[test]
fn get_order_status_encodes_path_and_maps_paid() {
    let server = MockServer::start();
    let order_id = "o-paidpaidpaidpaidpaidpaidpaidpaid";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"))
            .query_param("timeout_ms", "5000");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_status": "paid",
                "refunded": false,
                "refund_pending": false,
                "wired": true,
                "deposit_total": "KUDOS:1",
                "exchange_code": 0,
                "exchange_http_status": 0,
                "refund_amount": "KUDOS:0",
                "contract_terms": {
                    "summary": "Paid mock",
                    "amount": "KUDOS:1"
                },
                "last_payment": { "t_s": 1700000100 },
                "wire_details": [],
                "refund_details": [],
                "order_status_url": "https://example.test/orders/o-paid"
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let status = client
        .get_order_status(
            order_id,
            &StatusQuery {
                timeout_ms: Some(5000),
                session_id: None,
            },
        )
        .unwrap();

    mock.assert();
    assert!(status.status.is_paid());
    match status.status {
        MerchantOrderStatus::Paid(p) => {
            assert!(p.wired);
            assert_eq!(p.deposit_total.to_string(), "KUDOS:1");
        }
        other => panic!("expected paid, got {other:?}"),
    }
}

#[test]
fn not_found_maps_to_typed_error() {
    let server = MockServer::start();
    let deny = server.mock(|when, then| {
        when.method(GET)
            .path("/instances/sandbox/private/orders/missing-order");
        then.status(404)
            .header("content-type", "application/json")
            .json_body(json!({ "code": 52, "hint": "unknown order" }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .get_order_status("missing-order", &StatusQuery::default())
        .unwrap_err();
    deny.assert();
    match err {
        MerchantError::NotFound { status, hint } => {
            assert_eq!(status, 404);
            assert_eq!(hint.as_deref(), Some("unknown order"));
        }
        other => panic!("expected NotFound, got {other}"),
    }
}

#[test]
fn conflict_maps_to_typed_error() {
    let server = MockServer::start();
    let order_id = "o-dddddddddddddddddddddddddddddddd";
    let deny = server.mock(|when, then| {
        when.method(POST).path("/instances/sandbox/private/orders");
        then.status(409)
            .header("content-type", "application/json")
            .json_body(json!({
                "code": 53,
                "hint": "order already exists"
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .post_order(
            CreateOrderRequest::new(
                "dup",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap_err();
    deny.assert();
    match err {
        MerchantError::Conflict { status, hint } => {
            assert_eq!(status, 409);
            assert_eq!(hint.as_deref(), Some("order already exists"));
        }
        other => panic!("expected Conflict, got {other}"),
    }
}

#[test]
fn unauthorized_maps_to_typed_error() {
    let server = MockServer::start();

    let deny = server.mock(|when, then| {
        when.method(GET)
            .path("/instances/sandbox/private/orders/missing");
        then.status(401)
            .header("content-type", "application/json")
            .json_body(json!({
                "code": 50,
                "hint": "unauthorized"
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "wrong",
    )
    .unwrap();

    let err = client
        .get_order_status("missing", &StatusQuery::default())
        .unwrap_err();
    deny.assert();
    match err {
        MerchantError::Unauthorized { status, hint } => {
            assert_eq!(status, 401);
            assert_eq!(hint.as_deref(), Some("unauthorized"));
        }
        other => panic!("expected Unauthorized, got {other}"),
    }
}

#[test]
fn redirect_is_refused_and_not_followed() {
    let server = MockServer::start();
    let order_id = "o-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    let redirect = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"))
            .header("Authorization", "Bearer secret-token:sandbox");
        then.status(302)
            .header("Location", "https://evil.example/steal");
    });

    // If the client followed redirects with auth, this would be hit — it must not be.
    let evil = server.mock(|when, then| {
        when.method(GET).path("/steal");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "order_status": "unpaid" }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .get_order_status(order_id, &StatusQuery::default())
        .unwrap_err();
    redirect.assert();
    assert_eq!(evil.hits(), 0);
    match err {
        MerchantError::RedirectDisallowed { status, location } => {
            assert_eq!(status, 302);
            assert_eq!(location.as_deref(), Some("https://evil.example/steal"));
        }
        other => panic!("expected RedirectDisallowed, got {other}"),
    }
}

#[test]
fn create_order_rejects_empty_pay_uri() {
    let server = MockServer::start();
    let order_id = "o-ffffffffffffffffffffffffffffffff";

    let _post = server.mock(|when, then| {
        when.method(POST).path("/instances/sandbox/private/orders");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_id": order_id,
                "pay_deadline": { "t_s": 1893456000 }
            }));
    });

    let _status = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "order_status": "unpaid",
                "taler_pay_uri": "   ",
                "order_status_url": format!("https://example.test/orders/{order_id}"),
                "creation_time": { "t_s": 1700000000 }
            }));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .create_order(
            CreateOrderRequest::new(
                "empty-uri",
                Amount::parse("KUDOS:1").unwrap(),
                "https://example.com/thanks",
            )
            .with_order_id(order_id),
        )
        .unwrap_err();

    assert!(matches!(err, MerchantError::UnexpectedOrderStatus { .. }));
}

#[test]
fn malformed_json_200_is_protocol_error() {
    let server = MockServer::start();
    let deny = server.mock(|when, then| {
        when.method(GET).path("/instances/sandbox/config");
        then.status(200)
            .header("content-type", "application/json")
            .body("not-json{");
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client.get_config().unwrap_err();
    deny.assert();
    assert!(matches!(err, MerchantError::Protocol(_)));
}

#[test]
fn post_order_rejects_empty_summary_and_bad_fulfillment() {
    let server = MockServer::start();
    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let err = client
        .post_order(CreateOrderRequest::new(
            "   ",
            Amount::parse("KUDOS:1").unwrap(),
            "https://example.com/thanks",
        ))
        .unwrap_err();
    assert!(matches!(err, MerchantError::Config(_)));

    let err = client
        .post_order(CreateOrderRequest::new(
            "ok",
            Amount::parse("KUDOS:1").unwrap(),
            "javascript:alert(1)",
        ))
        .unwrap_err();
    assert!(matches!(err, MerchantError::Config(_)));
}

#[test]
fn get_order_status_encodes_session_id_query() {
    let server = MockServer::start();
    let order_id = "o-sessionidsessionidsessionidsessio";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/instances/sandbox/private/orders/{order_id}"))
            .query_param("session_id", "ab c");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(unpaid_body(order_id));
    });

    let client = MerchantClient::with_credentials(
        format!("{}/instances/sandbox/", server.base_url()),
        "sandbox",
    )
    .unwrap();

    let _ = client
        .get_order_status(
            order_id,
            &StatusQuery {
                timeout_ms: None,
                session_id: Some("ab c".into()),
            },
        )
        .unwrap();
    mock.assert();
}
