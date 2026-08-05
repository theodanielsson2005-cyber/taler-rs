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
  zeroized on drop.
- Prefer unguessable order IDs with `create_token: false` so public status URLs
  are not enumerable.
- Never put buyer PII in fulfillment URLs, logs, or scrapeable order listings.
