use rimelight_personal_core::finance::decode_finances;
use rimelight_personal_core::rates::{Rates, convert_to_brl};

fn rates(json: &str) -> Rates {
    serde_json::from_str(json).expect("fixture rates decode")
}

#[test]
fn decodes_accounts_and_bills_and_defaults_missing_lists() {
    let data = decode_finances(
        br#"{
            "accounts": [{"balance": 1500.5, "currency": "BRL", "name": "ignored"}],
            "bills": [{"cost": 10.0, "currency": "USD", "billingCycle": "monthly", "isActive": true}]
        }"#,
    )
    .expect("decodes");
    assert_eq!(data.accounts.len(), 1);
    assert_eq!(data.accounts[0].balance, 1500.5);
    assert_eq!(data.bills[0].billing_cycle, "monthly");
    assert!(data.bills[0].is_active);

    let empty = decode_finances(br#"{}"#).expect("decodes");
    assert!(empty.accounts.is_empty());
    assert!(empty.bills.is_empty());
}

#[test]
fn brl_passes_through_without_rates() {
    assert_eq!(convert_to_brl(42.0, "BRL", None), Some(42.0));
    assert_eq!(convert_to_brl(42.0, "", None), Some(42.0));
}

#[test]
fn foreign_amounts_divide_by_the_rate_per_brl() {
    // 1 BRL = 0.2 USD, so 100 USD = 500 BRL.
    let table = rates(r#"{"rates": {"USD": 0.2}}"#);
    assert_eq!(convert_to_brl(100.0, "USD", Some(&table)), Some(500.0));
    assert_eq!(convert_to_brl(100.0, "usd", Some(&table)), Some(500.0));
}

#[test]
fn missing_or_non_positive_rate_yields_none() {
    let table = rates(r#"{"rates": {"USD": 0.2, "EUR": 0.0}}"#);
    assert_eq!(convert_to_brl(100.0, "GBP", Some(&table)), None);
    assert_eq!(convert_to_brl(100.0, "EUR", Some(&table)), None);
    assert_eq!(convert_to_brl(100.0, "USD", None), None);
}
