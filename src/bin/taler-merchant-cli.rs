//! Minimal CLI for the taler-merchant PoC.
//!
//! Env:
//! - `TALER_MERCHANT_URL` — base URL including instance, e.g.
//!   `https://backend.demo.taler.net/instances/sandbox/`
//! - `TALER_MERCHANT_TOKEN` — `sandbox` or `secret-token:sandbox`

use std::env;
use std::process::ExitCode;

use taler_merchant::{Amount, CreateOrderRequest, MerchantClient, StatusQuery};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let cmd = args
        .next()
        .ok_or("usage: taler-merchant-cli <config|create-order|status> …")?;

    match cmd.as_str() {
        "config" => cmd_config()?,
        "create-order" => {
            let summary = args
                .next()
                .unwrap_or_else(|| "taler-rs PoC order".to_string());
            let amount = args.next().unwrap_or_else(|| "KUDOS:1".to_string());
            let fulfillment = args
                .next()
                .unwrap_or_else(|| "https://example.com/thanks".to_string());
            cmd_create_order(&summary, &amount, &fulfillment)?;
        }
        "status" => {
            let order_id = args
                .next()
                .ok_or("usage: taler-merchant-cli status <order_id> [timeout_ms]")?;
            let timeout_ms = match args.next() {
                None => None,
                Some(raw) => Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("invalid timeout_ms: {raw:?}"))?,
                ),
            };
            cmd_status(&order_id, timeout_ms)?;
        }
        "help" | "--help" | "-h" => print_help(),
        other => {
            return Err(format!("unknown command: {other}").into());
        }
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "\
taler-merchant-cli — GNU Taler Merchant Backend PoC

Commands:
  config
  create-order [summary] [amount] [fulfillment_url]
  status <order_id> [timeout_ms]

Environment:
  TALER_MERCHANT_URL    Base URL (instance included)
  TALER_MERCHANT_TOKEN  Bearer token material

Demo sandbox:
  TALER_MERCHANT_URL=https://backend.demo.taler.net/instances/sandbox/
  TALER_MERCHANT_TOKEN=sandbox
"
    );
}

fn client_from_env() -> Result<MerchantClient, Box<dyn std::error::Error>> {
    let url = env::var("TALER_MERCHANT_URL").map_err(|_| {
        "missing TALER_MERCHANT_URL (e.g. https://backend.demo.taler.net/instances/sandbox/)"
    })?;
    let token = env::var("TALER_MERCHANT_TOKEN")
        .map_err(|_| "missing TALER_MERCHANT_TOKEN (e.g. sandbox)")?;
    Ok(MerchantClient::with_credentials(url, token)?)
}

fn cmd_config() -> Result<(), Box<dyn std::error::Error>> {
    let client = client_from_env()?;
    let cfg = client.get_config()?;
    println!("name:            {}", cfg.name);
    println!("version:         {}", cfg.version);
    println!("currency:        {}", cfg.currency);
    if let Some(impl_urn) = &cfg.implementation {
        println!("implementation:  {impl_urn}");
    }
    if let Some(build) = &cfg.build_version {
        println!("build_version:   {build}");
    }
    Ok(())
}

fn cmd_create_order(
    summary: &str,
    amount: &str,
    fulfillment_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = client_from_env()?;
    let amount = Amount::parse(amount)?;
    let req = CreateOrderRequest::new(summary, amount, fulfillment_url);
    let created = client.create_order(req)?;

    println!("order_id:         {}", created.order_id);
    println!("order_status:     {}", created.order_status_str());
    println!("taler_pay_uri:    {}", created.taler_pay_uri());
    println!("order_status_url: {}", created.order_status_url());
    if created.token.is_some() {
        println!("claim_token:      [present — not printed]");
    }
    Ok(())
}

fn cmd_status(order_id: &str, timeout_ms: Option<u64>) -> Result<(), Box<dyn std::error::Error>> {
    let client = client_from_env()?;
    let status = client.get_order_status(
        order_id,
        &StatusQuery {
            timeout_ms,
            session_id: None,
        },
    )?;
    println!("order_id:         {}", status.order_id);
    println!("order_status:     {}", status.order_status());
    if let Some(uri) = status.taler_pay_uri() {
        println!("taler_pay_uri:    {uri}");
    }
    if let Some(url) = status.order_status_url() {
        println!("order_status_url: {url}");
    }
    if let Some(summary) = status.summary() {
        println!("summary:          {summary}");
    }
    if status.status.is_paid() {
        if let taler_merchant::MerchantOrderStatus::Paid(paid) = &status.status {
            println!("deposit_total:    {}", paid.deposit_total);
            println!("refunded:         {}", paid.refunded);
        }
    }
    Ok(())
}
