# API coverage map (PoC)

Honest inventory of Merchant Backend surface vs this crate.
Protocol reference: [api-merchant.html](https://docs.taler.net/core/api-merchant.html).
Demo `/config` observed around protocol `30:0:18`.

## Implemented

| Area | Endpoints / types | Notes |
|---|---|---|
| Config | `GET /config` | Required fields typed; extras retained via `flatten` |
| Orders write | `POST /private/orders` | Minimal order draft + `create_token`; weak IDs rejected if `create_token=false` |
| Orders read | `GET /private/orders/{id}` | Tagged union unpaid/claimed/paid |
| Create helper | `create_order` | POST+GET; unpaid + pay URI; orphan nests typed `cause` + `Error::source` |
| Amounts | common-API grammar | Letters-only currency ≤11; ≤8 frac; ≤2⁵² |
| Auth | Bearer `secret-token:` | Normalized; redacted; zeroized; Authorization header zeroized after send |
| HTTP client | ureq sync | **No redirects** (`RedirectDisallowed`); HTTPS required (HTTP loopback only) |
| CLI | config / create-order / status | Env-based sandbox smoke |

## Explicitly deferred (funded milestones)

| Area | Why deferred |
|---|---|
| Refunds (`POST …/refund`) | M2 |
| Order history / delete / forget | M1+ |
| Inventory / products / reserves | Out of horizontal SDK core for PoC |
| Login token (`/private/token`) | Useful; not required for sandbox password token |
| Webhooks | M2/M3 shop concerns |
| Async client (`async` feature) | After sync surface stabilizes |
| Reference shop | M3 |
| Public (unauthenticated) order endpoints | Wallet-facing; merchants use private API |
| Post-quantum protocol crypto | Exchange/wallet/core Taler — not merchant HTTP |

## Test evidence

- HTTP mocks: path, Bearer, body, create→status, orphan GET (+ `source`), unexpected claimed, empty pay URI, empty summary / bad fulfillment, claimed/paid/404/409/401/302-redirect, malformed JSON, session_id query encode, `create_token: true`
- Unit: Amount grammar + auth redaction/serde + URL join/encode + HTTPS/host/fulfillment rules + order-id entropy
- Fixtures: config / post / unpaid / claimed / paid
- Live (optional): public demo sandbox unpaid create
- CI: fmt + clippy `-D warnings` + test
