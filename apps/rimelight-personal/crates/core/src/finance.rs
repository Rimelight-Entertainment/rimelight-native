use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{ApiClient, ApiError, json_request, percent_encode};
use crate::rates::{Rates, convert_to_brl};

/// Everything `GET /api/finances` returns. Missing lists decode as empty, as the Solid app treats them.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceData {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub budgets: Vec<Budget>,
    #[serde(default)]
    pub bills: Vec<Bill>,
    #[serde(default)]
    pub planned_purchases: Vec<PlannedPurchase>,
    #[serde(default)]
    pub debts: Vec<Debt>,
}

/// A financial account. `balance` is in `currency`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// `checking`, `savings`, `credit`, `investment`, or `cash`.
    #[serde(default, rename = "type")]
    pub account_type: String,
    #[serde(default)]
    pub balance: f64,
    #[serde(default)]
    pub currency: String,
    pub institution: Option<String>,
    pub agency: Option<String>,
    pub account_number: Option<String>,
    pub payment_key: Option<String>,
    pub credit_limit: Option<f64>,
    pub notes: Option<String>,
}

/// A ledger entry. `kind` is `income`, `expense`, or `transfer`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    #[serde(default)]
    pub id: String,
    pub account_id: Option<String>,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub category: String,
    pub description: Option<String>,
    /// `YYYY-MM-DD` (the server may append a time).
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub is_prepaid: bool,
    pub prepaid_months: Option<u32>,
    pub recognized_monthly_amount: Option<f64>,
    pub prepaid_expires_at: Option<String>,
}

impl Transaction {
    pub fn is_income(&self) -> bool {
        self.kind == "income"
    }
}

/// A monthly spending limit for one category.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub monthly_limit: f64,
    #[serde(default)]
    pub spent: f64,
    #[serde(default)]
    pub currency: String,
}

/// A recurring bill. `cost` is per `billing_cycle` (`monthly`, `yearly`, or `weekly`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bill {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub cost: f64,
    pub currency: String,
    pub billing_cycle: String,
    #[serde(default)]
    pub category: String,
    pub renewal_date: Option<String>,
    pub payment_method: Option<String>,
    pub is_active: bool,
    pub notes: Option<String>,
}

impl Bill {
    /// The cost normalised to one month, in `currency`.
    pub fn monthly_cost(&self) -> f64 {
        match self.billing_cycle.as_str() {
            "yearly" => self.cost / 12.0,
            "weekly" => self.cost * 52.0 / 12.0,
            _ => self.cost,
        }
    }
}

/// A wishlist item. `priority` is `low`, `medium`, `high`, or `essential`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedPurchase {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub estimated_cost: Option<f64>,
    pub quantity: Option<f64>,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub priority: String,
    pub target_date: Option<String>,
    pub link: Option<String>,
    #[serde(default)]
    pub is_purchased: bool,
    pub notes: Option<String>,
}

impl PlannedPurchase {
    /// Estimated cost times quantity, in `currency`. Zero when no estimate is set.
    pub fn item_total(&self) -> f64 {
        self.estimated_cost.unwrap_or(0.0) * self.quantity.unwrap_or(1.0)
    }
}

/// A loan or liability. `direction` is `borrowed` or `lent`; `debt_type` is the liability kind.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Debt {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub direction: Option<String>,
    #[serde(default, rename = "type")]
    pub debt_type: String,
    pub lender: Option<String>,
    #[serde(default)]
    pub original_amount: f64,
    #[serde(default)]
    pub remaining_amount: f64,
    #[serde(default)]
    pub currency: String,
    pub interest_rate: Option<f64>,
    pub interest_period: Option<String>,
    pub installment_amount: Option<f64>,
    pub total_installments: Option<u32>,
    pub remaining_installments: Option<u32>,
    pub due_date_day: Option<u32>,
    pub end_date: Option<String>,
    pub linked_account_id: Option<String>,
    #[serde(default)]
    pub is_paid: bool,
    pub notes: Option<String>,
}

impl Debt {
    /// Money lent to someone else (a receivable), not a liability.
    pub fn is_lent(&self) -> bool {
        self.direction.as_deref() == Some("lent") || self.debt_type == "loan_given"
    }
}

pub fn decode_finances(bytes: &[u8]) -> Result<FinanceData, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

// ---------------------------------------------------------------------------
// Dates. Dates are `YYYY-MM-DD` strings; only the first ten characters matter.
// ---------------------------------------------------------------------------

/// The `YYYY-MM` prefix of a date, or `""` when the date is too short.
pub fn month_key(date: &str) -> &str {
    date.get(..7).unwrap_or("")
}

fn parse_ymd(value: &str) -> Option<(i64, i64, i64)> {
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<i64>().ok()?;
    let day = value.get(8..10)?.parse::<i64>().ok()?;
    let well_formed =
        value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-');
    (well_formed && (1..=12).contains(&month) && (1..=31).contains(&day))
        .then_some((year, month, day))
}

/// Days since 1970-01-01 (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Whole days from `from` to `to`. `None` when either is not a `YYYY-MM-DD` date.
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    let (fy, fm, fd) = parse_ymd(from)?;
    let (ty, tm, td) = parse_ymd(to)?;
    Some(days_from_civil(ty, tm, td) - days_from_civil(fy, fm, fd))
}

// ---------------------------------------------------------------------------
// Totals. Foreign-currency amounts are converted to BRL; without rates they pass through.
// ---------------------------------------------------------------------------

fn to_brl(amount: f64, currency: &str, rates: Option<&Rates>) -> f64 {
    convert_to_brl(amount, currency, rates).unwrap_or(amount)
}

/// The dashboard-level totals in BRL, as the Solid `FinancesContext` computes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub total_balance: f64,
    /// Unpaid money others owe you.
    pub total_lent: f64,
    /// Unpaid liabilities you owe.
    pub total_borrowed: f64,
    pub net_worth: f64,
    /// Active bills normalised to a month.
    pub monthly_burn: f64,
    /// Installments on unpaid liabilities.
    pub monthly_debt_payment: f64,
    /// Income recognised this month, including prepaid spreads.
    pub monthly_income: f64,
    /// Estimated cost of wishlist items not yet bought.
    pub total_wishlist: f64,
    pub wishlist_count: usize,
}

/// `today` is the local `YYYY-MM-DD`; it selects the month for income and spending.
pub fn summarize(data: &FinanceData, rates: Option<&Rates>, today: &str) -> Summary {
    let month = month_key(today);
    let mut s = Summary::default();
    for account in &data.accounts {
        s.total_balance += to_brl(account.balance, &account.currency, rates);
    }
    for debt in data.debts.iter().filter(|d| !d.is_paid) {
        let amount = to_brl(debt.remaining_amount, &debt.currency, rates);
        if debt.is_lent() {
            s.total_lent += amount;
        } else {
            s.total_borrowed += amount;
        }
    }
    s.net_worth = s.total_balance + s.total_lent - s.total_borrowed;
    s.monthly_burn = data
        .bills
        .iter()
        .filter(|b| b.is_active)
        .map(|b| to_brl(b.monthly_cost(), &b.currency, rates))
        .sum();
    s.monthly_debt_payment = data
        .debts
        .iter()
        .filter(|d| !d.is_paid && !d.is_lent())
        .filter_map(|d| d.installment_amount.map(|a| to_brl(a, &d.currency, rates)))
        .sum();
    s.monthly_income = data
        .transactions
        .iter()
        .filter(|t| t.is_income())
        .filter_map(|t| recognized_in_month(t, month).map(|a| to_brl(a, &t.currency, rates)))
        .sum();
    for plan in data.planned_purchases.iter().filter(|p| !p.is_purchased) {
        s.total_wishlist += to_brl(plan.item_total(), &plan.currency, rates);
        s.wishlist_count += 1;
    }
    s
}

/// The part of an income that counts toward `month` (`YYYY-MM`), in the transaction's currency.
/// A prepaid income counts its monthly share from its start month up to, not including, its expiry month.
fn recognized_in_month(t: &Transaction, month: &str) -> Option<f64> {
    if t.is_prepaid
        && let (Some(recognized), Some(expires)) =
            (t.recognized_monthly_amount, t.prepaid_expires_at.as_deref())
    {
        let start = month_key(&t.date);
        let end = month_key(expires);
        return (start <= month && month < end).then_some(recognized);
    }
    (month_key(&t.date) == month).then_some(t.amount)
}

/// Spending this month for a budget, in the budget's currency. Falls back to `budget.spent`
/// when no expense this month matches the category.
pub fn budget_spent(data: &FinanceData, budget: &Budget, month: &str) -> f64 {
    let mut found = false;
    let mut total = 0.0;
    for t in data
        .transactions
        .iter()
        .filter(|t| !t.is_income() && t.category == budget.category && month_key(&t.date) == month)
    {
        found = true;
        total += t.amount;
    }
    if found { total } else { budget.spent }
}

/// A payment due within the next 30 days.
#[derive(Debug, Clone, PartialEq)]
pub struct Upcoming {
    pub name: String,
    pub amount: f64,
    pub currency: String,
    pub category: String,
    pub days: i64,
}

/// Active bills renewing and unpaid liabilities with an installment due within 30 days,
/// sorted soonest first.
pub fn upcoming(data: &FinanceData, today: &str) -> Vec<Upcoming> {
    let mut out = Vec::new();
    for bill in data.bills.iter().filter(|b| b.is_active) {
        let Some(renewal) = bill.renewal_date.as_deref() else {
            continue;
        };
        let Some(days) = days_between(today, renewal) else {
            continue;
        };
        if (0..=30).contains(&days) {
            out.push(Upcoming {
                name: bill.name.clone(),
                amount: bill.cost,
                currency: bill.currency.clone(),
                category: bill.category.clone(),
                days,
            });
        }
    }
    if let Some((year, month, day)) = parse_ymd(today) {
        let today_num = days_from_civil(year, month, day);
        for debt in data.debts.iter().filter(|d| !d.is_paid && !d.is_lent()) {
            let (Some(amount), Some(due)) = (debt.installment_amount, debt.due_date_day) else {
                continue;
            };
            let due = i64::from(due);
            let mut due_date = days_from_civil(year, month, due.min(days_in_month(year, month)));
            if due_date < today_num {
                let (ny, nm) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };
                due_date = days_from_civil(ny, nm, due.min(days_in_month(ny, nm)));
            }
            let days = due_date - today_num;
            if (0..=30).contains(&days) {
                out.push(Upcoming {
                    name: format!("{} (Loan Installment)", debt.name),
                    amount,
                    currency: debt.currency.clone(),
                    category: "Debt Repayment".to_owned(),
                    days,
                });
            }
        }
    }
    out.sort_by_key(|u| u.days);
    out
}

// ---------------------------------------------------------------------------
// Writes. Bodies match the Solid `FinancesContext` and modal payloads.
// ---------------------------------------------------------------------------

/// The four collections that `DELETE /api/finances` removes by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinanceKind {
    Account,
    Transaction,
    Budget,
    Bill,
    Plan,
    Debt,
}

impl FinanceKind {
    fn query_key(self) -> &'static str {
        match self {
            Self::Account => "accountId",
            Self::Transaction => "txId",
            Self::Budget => "budgetId",
            Self::Bill => "billId",
            Self::Plan => "planId",
            Self::Debt => "debtId",
        }
    }
}

/// Form values for an account. Empty text is sent as `null`.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountInput {
    pub name: String,
    pub account_type: String,
    pub balance: f64,
    pub currency: String,
    pub institution: String,
    pub agency: String,
    pub account_number: String,
    pub payment_key: String,
    pub credit_limit: Option<f64>,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxKind {
    Income,
    Expense,
}

/// Prepaid income: the total is recognised as `recognized_monthly` for `months` months.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepaid {
    pub months: u32,
    pub recognized_monthly: f64,
    pub expires_at: String,
}

/// Form values for a new transaction, or for editing one.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionInput {
    pub kind: TxKind,
    pub description: String,
    pub amount: f64,
    pub currency: String,
    pub category: String,
    pub account_id: Option<String>,
    pub date: String,
    /// Only used when creating a prepaid income. Ignored for edits and expenses.
    pub prepaid: Option<Prepaid>,
}

/// Form values for a recurring bill.
#[derive(Debug, Clone, PartialEq)]
pub struct BillInput {
    pub name: String,
    pub cost: f64,
    pub currency: String,
    pub billing_cycle: String,
    pub category: String,
    pub renewal_date: String,
    pub payment_method: String,
    pub notes: String,
}

/// Form values for a new liability or loan given.
#[derive(Debug, Clone, PartialEq)]
pub struct DebtInput {
    pub lent: bool,
    /// Liability type, used only when `lent` is false. Ignored when `lent` is true.
    pub debt_type: String,
    pub name: String,
    pub lender: String,
    pub original_amount: f64,
    pub remaining_amount: f64,
    pub currency: String,
    pub interest_rate: Option<f64>,
    pub installment_amount: Option<f64>,
    pub total_installments: Option<u32>,
    pub remaining_installments: Option<u32>,
    pub due_date_day: Option<u32>,
    pub end_date: String,
    pub linked_account_id: Option<String>,
    pub notes: String,
}

/// Form values for a wishlist item.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanInput {
    pub title: String,
    pub estimated_cost: Option<f64>,
    pub quantity: f64,
    pub currency: String,
    pub category: String,
    pub priority: String,
    pub target_date: String,
    pub link: String,
    pub notes: String,
}

fn opt_text(value: &str) -> Value {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Value::Null
    } else {
        Value::String(trimmed.to_owned())
    }
}

fn opt_id(value: &Option<String>) -> Value {
    value
        .as_deref()
        .map_or(Value::Null, |id| Value::String(id.to_owned()))
}

fn account_body(input: &AccountInput) -> Value {
    json!({
        "type": "account",
        "name": input.name.trim(),
        "accountType": input.account_type,
        "balance": input.balance,
        "currency": input.currency,
        "institution": opt_text(&input.institution),
        "agency": opt_text(&input.agency),
        "accountNumber": opt_text(&input.account_number),
        "paymentKey": opt_text(&input.payment_key),
        "creditLimit": input.credit_limit,
        "notes": opt_text(&input.notes),
    })
}

fn bill_fields(input: &BillInput) -> Value {
    json!({
        "name": input.name.trim(),
        "cost": input.cost,
        "currency": input.currency,
        "billingCycle": input.billing_cycle,
        "category": input.category,
        "renewalDate": opt_text(&input.renewal_date),
        "paymentMethod": opt_text(&input.payment_method),
    })
}

fn plan_fields(input: &PlanInput) -> Value {
    json!({
        "title": input.title.trim(),
        "estimatedCost": input.estimated_cost,
        "quantity": input.quantity,
        "currency": input.currency,
        "category": input.category,
        "priority": input.priority,
        "targetDate": opt_text(&input.target_date),
        "link": opt_text(&input.link),
        "notes": opt_text(&input.notes),
    })
}

fn with_type_and_id(kind: &str, id: Option<&str>, mut fields: Value) -> Value {
    if let Value::Object(map) = &mut fields {
        map.insert("type".to_owned(), Value::String(kind.to_owned()));
        if let Some(id) = id {
            map.insert("id".to_owned(), Value::String(id.to_owned()));
        }
    }
    fields
}

fn merge(mut base: Value, extra: Value) -> Value {
    if let (Value::Object(base_map), Value::Object(extra_map)) = (&mut base, extra) {
        base_map.extend(extra_map);
    }
    base
}

/// Body for `POST /api/finances` from the global quick-transaction modal. Only these keys are sent.
/// `account_id` and `date` follow the Solid body: `accountId` is `null` when unset, `date` is omitted when empty.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewQuickTransaction<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub description: &'a str,
    pub amount: f64,
    pub tx_type: &'a str,
    pub currency: &'a str,
    pub category: &'a str,
    pub account_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<&'a str>,
}

impl ApiClient {
    /// `PATCH` when `id` is set, `POST` otherwise.
    fn write_finance(
        &self,
        method: Method,
        body: Value,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let bytes = serde_json::to_vec(&body)
            .expect("request bodies are plain values and always serialize");
        self.send(
            json_request(method, self.url("/finances"), bytes),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// `DELETE /api/finances?<kind>=<id>`.
    pub fn delete_finance(
        &self,
        kind: FinanceKind,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = format!(
            "{}?{}={}",
            self.url("/finances"),
            kind.query_key(),
            percent_encode(id)
        );
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Creates an account, or updates the one with `id`.
    pub fn save_account(
        &self,
        id: Option<&str>,
        input: &AccountInput,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id("account", id, account_body(input));
        let method = if id.is_some() {
            Method::PATCH
        } else {
            Method::POST
        };
        self.write_finance(method, body, on_done);
    }

    /// Creates a transaction, or updates the one with `id`.
    /// Edits send `currency` only for incomes and never send `txType`.
    pub fn save_transaction(
        &self,
        id: Option<&str>,
        input: &TransactionInput,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let common = json!({
            "description": input.description.trim(),
            "amount": input.amount,
            "category": input.category,
            "accountId": opt_id(&input.account_id),
            "date": input.date,
        });
        let body = match id {
            Some(id) => {
                let mut body = with_type_and_id("transaction", Some(id), common);
                if input.kind == TxKind::Income
                    && let Value::Object(map) = &mut body
                {
                    map.insert("currency".to_owned(), Value::String(input.currency.clone()));
                }
                (Method::PATCH, body)
            }
            None => {
                let mut fields = merge(
                    common,
                    json!({
                        "txType": if input.kind == TxKind::Income { "income" } else { "expense" },
                        "currency": input.currency,
                    }),
                );
                if input.kind == TxKind::Income {
                    let prepaid = match &input.prepaid {
                        Some(p) => json!({
                            "isPrepaid": true,
                            "prepaidMonths": p.months,
                            "recognizedMonthlyAmount": p.recognized_monthly,
                            "prepaidExpiresAt": p.expires_at,
                        }),
                        None => json!({
                            "isPrepaid": false,
                            "prepaidMonths": null,
                            "recognizedMonthlyAmount": null,
                            "prepaidExpiresAt": null,
                        }),
                    };
                    fields = merge(fields, prepaid);
                }
                (Method::POST, with_type_and_id("transaction", None, fields))
            }
        };
        self.write_finance(body.0, body.1, on_done);
    }

    /// `POST /api/finances` with the global quick-transaction modal's body. The caller refreshes finances.
    pub fn create_quick_transaction(
        &self,
        tx: &NewQuickTransaction<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let bytes =
            serde_json::to_vec(tx).expect("request bodies are plain structs and always serialize");
        self.send(
            json_request(Method::POST, self.url("/finances"), bytes),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Creates a recurring bill (active) or updates the one with `id`.
    pub fn save_bill(
        &self,
        id: Option<&str>,
        input: &BillInput,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = match id {
            Some(id) => {
                let fields = merge(
                    bill_fields(input),
                    json!({ "notes": opt_text(&input.notes) }),
                );
                (Method::PATCH, with_type_and_id("bill", Some(id), fields))
            }
            None => {
                let fields = merge(bill_fields(input), json!({ "isActive": true }));
                (Method::POST, with_type_and_id("bill", None, fields))
            }
        };
        self.write_finance(body.0, body.1, on_done);
    }

    pub fn set_bill_active(
        &self,
        id: &str,
        is_active: bool,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id("bill", Some(id), json!({ "isActive": is_active }));
        self.write_finance(Method::PATCH, body, on_done);
    }

    pub fn create_budget(
        &self,
        category: &str,
        monthly_limit: f64,
        currency: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id(
            "budget",
            None,
            json!({ "category": category, "monthlyLimit": monthly_limit, "currency": currency }),
        );
        self.write_finance(Method::POST, body, on_done);
    }

    pub fn save_budget_limit(
        &self,
        id: &str,
        monthly_limit: f64,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id("budget", Some(id), json!({ "monthlyLimit": monthly_limit }));
        self.write_finance(Method::PATCH, body, on_done);
    }

    pub fn create_debt(
        &self,
        input: &DebtInput,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let (direction, debt_type) = if input.lent {
            ("lent", "loan_given")
        } else {
            ("borrowed", input.debt_type.as_str())
        };
        let body = with_type_and_id(
            "debt",
            None,
            json!({
                "name": input.name.trim(),
                "direction": direction,
                "debtType": debt_type,
                "lender": opt_text(&input.lender),
                "originalAmount": input.original_amount,
                "remainingAmount": input.remaining_amount,
                "currency": input.currency,
                "interestRate": input.interest_rate,
                "interestPeriod": "yearly",
                "installmentAmount": input.installment_amount,
                "totalInstallments": input.total_installments,
                "remainingInstallments": input.remaining_installments,
                "dueDateDay": input.due_date_day,
                "endDate": opt_text(&input.end_date),
                "linkedAccountId": opt_id(&input.linked_account_id),
                "notes": opt_text(&input.notes),
            }),
        );
        self.write_finance(Method::POST, body, on_done);
    }

    /// Marks a liability or loan as settled (`true`) or reopens it (`false`).
    pub fn set_debt_paid(
        &self,
        id: &str,
        is_paid: bool,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id("debt", Some(id), json!({ "isPaid": is_paid }));
        self.write_finance(Method::PATCH, body, on_done);
    }

    /// Records a payment on `debt`: lowers its balance, then posts the matching transaction.
    /// A lent debt posts an income; anything else posts an expense. Stops after the first failure.
    pub fn record_debt_payment(
        &self,
        debt: &Debt,
        amount: f64,
        account_id: Option<String>,
        date: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let remaining = (debt.remaining_amount - amount).max(0.0);
        let installments = debt.remaining_installments.map(|n| n.saturating_sub(1));
        let patch = with_type_and_id(
            "debt",
            Some(&debt.id),
            json!({
                "remainingAmount": remaining,
                "remainingInstallments": installments,
                "isPaid": remaining == 0.0,
            }),
        );
        let (tx_type, description, category) = if debt.is_lent() {
            let from = debt
                .lender
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&debt.name);
            (
                "income",
                format!("Loan repayment received from: {from}"),
                "Investments & Dividends",
            )
        } else {
            (
                "expense",
                format!("Payment for loan: {}", debt.name),
                "Debt & Interest Payments",
            )
        };
        let tx = with_type_and_id(
            "transaction",
            None,
            json!({
                "txType": tx_type,
                "description": description,
                "amount": amount,
                "currency": debt.currency,
                "category": category,
                "accountId": account_id,
                "date": date,
            }),
        );
        let client = self.clone();
        self.write_finance(Method::PATCH, patch, move |result| match result {
            Err(e) => on_done(Err(e)),
            Ok(()) => client.write_finance(Method::POST, tx, on_done),
        });
    }

    /// Adds a wishlist item (not purchased) or updates the one with `id`.
    pub fn save_plan(
        &self,
        id: Option<&str>,
        input: &PlanInput,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = match id {
            Some(id) => (
                Method::PATCH,
                with_type_and_id("plannedPurchase", Some(id), plan_fields(input)),
            ),
            None => {
                let fields = merge(plan_fields(input), json!({ "isPurchased": false }));
                (
                    Method::POST,
                    with_type_and_id("plannedPurchase", None, fields),
                )
            }
        };
        self.write_finance(body.0, body.1, on_done);
    }

    pub fn set_plan_purchased(
        &self,
        id: &str,
        is_purchased: bool,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = with_type_and_id(
            "plannedPurchase",
            Some(id),
            json!({ "isPurchased": is_purchased }),
        );
        self.write_finance(Method::PATCH, body, on_done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bill(cycle: &str, cost: f64) -> Bill {
        Bill {
            cost,
            currency: "BRL".into(),
            billing_cycle: cycle.into(),
            is_active: true,
            ..Default::default()
        }
    }

    #[test]
    fn decodes_partial_payload_with_missing_lists() {
        let data = decode_finances(br#"{"accounts":[{"balance":10.5,"currency":"BRL"}]}"#).unwrap();
        assert_eq!(data.accounts.len(), 1);
        assert!(data.bills.is_empty() && data.debts.is_empty());
    }

    #[test]
    fn normalises_bill_cycles() {
        assert_eq!(bill("monthly", 30.0).monthly_cost(), 30.0);
        assert_eq!(bill("yearly", 1200.0).monthly_cost(), 100.0);
        assert!((bill("weekly", 10.0).monthly_cost() - 520.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn counts_days_across_leap_february() {
        assert_eq!(days_between("2024-01-01", "2024-03-01"), Some(60));
        assert_eq!(days_between("2024-03-05", "2024-03-05"), Some(0));
        assert_eq!(days_between("nope", "2024-03-05"), None);
    }

    #[test]
    fn summarises_balances_and_debts() {
        let data = FinanceData {
            accounts: vec![Account {
                balance: 1000.0,
                currency: "BRL".into(),
                ..Default::default()
            }],
            debts: vec![
                Debt {
                    name: "owed".into(),
                    remaining_amount: 200.0,
                    currency: "BRL".into(),
                    ..Default::default()
                },
                Debt {
                    name: "lent".into(),
                    direction: Some("lent".into()),
                    remaining_amount: 300.0,
                    currency: "BRL".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let s = summarize(&data, None, "2024-03-05");
        assert_eq!(s.total_balance, 1000.0);
        assert_eq!(s.total_borrowed, 200.0);
        assert_eq!(s.total_lent, 300.0);
        assert_eq!(s.net_worth, 1100.0);
    }

    #[test]
    fn spreads_prepaid_income_over_its_months() {
        let tx = Transaction {
            amount: 600.0,
            currency: "BRL".into(),
            kind: "income".into(),
            date: "2024-01-10".into(),
            is_prepaid: true,
            recognized_monthly_amount: Some(100.0),
            prepaid_expires_at: Some("2024-07-10".into()),
            ..Default::default()
        };
        assert_eq!(recognized_in_month(&tx, "2024-03"), Some(100.0));
        assert_eq!(recognized_in_month(&tx, "2024-07"), None);
        assert_eq!(recognized_in_month(&tx, "2023-12"), None);
    }

    #[test]
    fn lists_bills_renewing_within_thirty_days() {
        let data = FinanceData {
            bills: vec![Bill {
                name: "Netflix".into(),
                renewal_date: Some("2024-03-10".into()),
                ..bill("monthly", 40.0)
            }],
            ..Default::default()
        };
        let up = upcoming(&data, "2024-03-05");
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].days, 5);
    }
}
