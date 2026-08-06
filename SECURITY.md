# Security Policy

## Supported versions

This repository is a pre-funding proof of concept. Security fixes are applied
on the default branch as promptly as possible.

## Reporting a vulnerability

**Do not open a public issue for security-sensitive reports.**

Please report privately via one of:

1. GitHub Security Advisories on
   [theodanielsson2005-cyber/taler-rs](https://github.com/theodanielsson2005-cyber/taler-rs/security/advisories/new)
2. Or contact the maintainer through GitHub:
   [@theodanielsson2005-cyber](https://github.com/theodanielsson2005-cyber)

Include steps to reproduce, affected commit/version, and impact assessment if
possible. We will acknowledge receipt and work on a fix as promptly as we can.

## Security-relevant design notes (PoC)

- Merchant access tokens and claim tokens are redacted in `Debug`/`Display` and
  zeroized on drop. `ClaimToken` **Serialize** also emits `[REDACTED]` so
  accidental JSON logging cannot leak secrets — use `ClaimToken::as_str()` only
  for intentional persistence / pay-URI construction.
- HTTP redirects are **never followed** (`redirects(0)`); a 3xx response becomes
  `MerchantError::RedirectDisallowed` so Bearer tokens cannot hop to another host.
- `base_url` must be `https://` with a host, or `http://` only for loopback
  (`127.0.0.1` / `localhost` / `::1`) used by local tests.
- `fulfillment_url` must be an absolute `http`/`https` URL (rejects `javascript:` etc.).
- Prefer unguessable order IDs with `create_token: false` so public status URLs
  are not enumerable.
- Never put buyer PII in fulfillment URLs, logs, or scrapeable order listings.
