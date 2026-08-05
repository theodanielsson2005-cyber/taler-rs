#!/usr/bin/env bash
# Smoke demo against the public Taler merchant sandbox.
set -euo pipefail

export TALER_MERCHANT_URL="${TALER_MERCHANT_URL:-https://backend.demo.taler.net/instances/sandbox/}"
export TALER_MERCHANT_TOKEN="${TALER_MERCHANT_TOKEN:-sandbox}"

echo "== config =="
cargo run -q --bin taler-merchant-cli -- config

echo "== create-order =="
out="$(cargo run -q --bin taler-merchant-cli -- create-order "taler-rs smoke" KUDOS:1 https://example.com/thanks)"
echo "$out"

order_id="$(printf '%s\n' "$out" | awk '/^order_id:/{print $2}')"
test -n "$order_id"

echo "== status =="
cargo run -q --bin taler-merchant-cli -- status "$order_id"

echo "OK — scan taler_pay_uri from create-order output with a Taler wallet."
