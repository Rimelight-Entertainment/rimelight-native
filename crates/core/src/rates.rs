//! FX rates for the dashboard: the same Frankfurter query the Solid app makes.

use std::collections::BTreeMap;

use ehttp::Request;
use serde::Deserialize;

use crate::api::{ApiError, send_anonymous};

/// Every amount is converted into this currency.
pub const BASE_CURRENCY: &str = "BRL";

pub const RATES_URL: &str = "https://api.frankfurter.dev/v1/latest?from=BRL&to=USD,CAD,GBP,EUR,RON";

/// `rates[code]` is the amount of `code` per one BRL.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rates {
    pub rates: BTreeMap<String, f64>,
}

fn decode_rates(bytes: &[u8]) -> Result<Rates, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

/// Calls `on_done` with the rates, or with `None` on any failure. The dashboard then
/// shows raw amounts, as the Solid app does.
pub fn fetch_rates(on_done: impl FnOnce(Option<Rates>) + Send + 'static) {
    send_anonymous(Request::get(RATES_URL), decode_rates, move |result| {
        on_done(result.ok());
    });
}

/// Converts `amount` of `currency` into BRL. `None` when the rate is missing or not positive.
/// An empty currency counts as BRL.
pub fn convert_to_brl(amount: f64, currency: &str, rates: Option<&Rates>) -> Option<f64> {
    let from = if currency.is_empty() {
        BASE_CURRENCY.to_owned()
    } else {
        currency.to_uppercase()
    };
    if from == BASE_CURRENCY {
        return Some(amount);
    }
    let rate = rates?.rates.get(&from).copied()?;
    if rate <= 0.0 {
        return None;
    }
    Some(amount / rate)
}
