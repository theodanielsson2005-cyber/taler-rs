# taler-rs

Rust client library + CLI for the [GNU Taler](https://taler.net/) **Merchant Backend** HTTP API.

Part of an **NGI TALER** proposal. This repository is a **pre-funding happy-path proof of concept** — not a crates.io release and not a full Merchant API SDK.

| | |
|---|---|
| Crate | `taler-merchant` |
| License | Apache-2.0 |
| Status | Happy-path PoC (`/config`, create order, payment status) |
| Demo backend | `https://backend.demo.taler.net/instances/sandbox/` |

## Why this exists

FOSS projects often hand-roll Merchant Backend HTTP glue. Rust backends especially lack a maintained typed client. This crate is a **horizontal** building block (not another CMS plugin): create orders, query payment status, handle amounts safely, and default to privacy-preserving order references.

Out of scope for now: wallet/exchange clients, ERP plugins, reference shop (funded milestone), post-quantum protocol work (core Taler / exchange+wallet).

## What is implemented

| Endpoint | Client API |
|---|---|
| `GET /config` | `MerchantClient::get_config` |
| `POST /private/orders` | `MerchantClient::post_order` / `create_order` |
| `GET /private/orders/{id}` | `MerchantClient::get_order_status` / `get_order_status_raw` |

Also:

- Spec-faithful `Amount` (no `f64`; currency ≤11 letters; ≤8 fractional digits; integer ≤2⁵²)
- Typed status union: **unpaid / claimed / paid**
- Secret redaction + zeroize; `ClaimToken` redacts on `Serialize` too
- Unguessable order-id helper; weak IDs rejected when `create_token: false`
- `create_order` requires unpaid + pay URI; typed orphan error if POST ok / GET fails
- Offline fixtures + HTTP mock contract tests + optional live sandbox test
- CI: `fmt` · `clippy -D warnings` · `test`

Honest inventory: [docs/API_COVERAGE.md](docs/API_COVERAGE.md).

## Privacy defaults

1. **Unguessable order IDs** — `generate_order_id()` + `create_token: false` (weak custom IDs are rejected)
2. **Secret redaction** — tokens never appear in `Debug` / `Display` / accidental JSON serialize of claim tokens
3. **Adopter guidance** — no buyer PII in fulfillment URLs/logs; no scrapeable public order listings

## Quick start (CLI)

```bash
export TALER_MERCHANT_URL=https://backend.demo.taler.net/instances/sandbox/
export TALER_MERCHANT_TOKEN=sandbox

cargo run --bin taler-merchant-cli -- config
cargo run --bin taler-merchant-cli -- create-order "Donation" KUDOS:1 https://example.com/thanks
cargo run --bin taler-merchant-cli -- status <order_id>
```

Or: `bash scripts/smoke.sh`

Scan / open the printed `taler_pay_uri` with a [Taler wallet](https://wallet.taler.net/) funded from [bank.demo.taler.net](https://bank.demo.taler.net/).

## Library sketch

```rust
use taler_merchant::{Amount, CreateOrderRequest, MerchantClient, StatusQuery};

let client = MerchantClient::with_credentials(
    "https://backend.demo.taler.net/instances/sandbox/",
    "sandbox",
)?;

let cfg = client.get_config()?;
assert_eq!(cfg.name, "taler-merchant");

// create_order = POST + GET status; Ok => unpaid with taler_pay_uri
let created = client.create_order(CreateOrderRequest::new(
    "Donation",
    Amount::parse("KUDOS:1")?,
    "https://example.com/thanks",
))?;

println!("pay: {}", created.taler_pay_uri());
let status = client.get_order_status(&created.order_id, &StatusQuery::default())?;
assert_eq!(status.order_status(), "unpaid");
```

Use `MerchantClient::post_order` if you only want the create response without the follow-up GET.

## Tests

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --test integration -- --ignored   # live sandbox (network)
```

## Protocol / positioning

- Specs: [Merchant Backend API](https://docs.taler.net/core/api-merchant.html), [Amounts](https://docs.taler.net/core/api-common.html#amounts)
- Demo `/config` currently reports protocol version around `30:x:y`
- Related GNU Taler Rust work (`taler-rust`) focuses on bank/wire adapters; this crate targets **merchant integration** for application developers. Naming and amount rules follow upstream Taler conventions; coordination via the Taler Integration Community Hub is intended for funded work.

## Funded roadmap (not this PoC)

- M1 completeness (broader client surface, more fixtures)
- M2 refunds & payment helpers
- M3 minimal reference shop
- M4 docs + tagged v0.1
- M5 hardening / expanded coverage map

## License

Apache-2.0 — see [LICENSE](LICENSE).
