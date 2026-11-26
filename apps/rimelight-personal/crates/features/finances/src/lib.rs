//! The Finances page controller (ported from `crates/app/src/finances.rs`; the behaviour comes from the web app's
//! `src/components/finances/` in personal.idantity.me).
//!
//! Plain Rust with no Slint. [`FinancesController`] holds the page state (the finance data, the FX rates, the active
//! tab, the filters and the open form), calls the endpoints through [`FinanceApi`], and reports each state change as a
//! [`FinancesView`]. The Slint glue in `apps/desktop/src/pages/finances.rs` turns the views into the models of
//! `ui/finances.slint`.
//!
//! Threading follows the Todos page: the API calls return at once and answer later on a worker thread. The controller
//! never holds its lock across a call, and `notify` runs after the lock is released.
//!
//! Row ids are `<prefix>:<id>`, where the prefix names the collection: `account`, `tx`, `bill`, `budget`, `debt` or
//! `plan`. The markup sends the row id back for the row's actions.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rimelight_personal_core::finance::{
    Account, AccountInput, Bill, BillInput, Budget, Debt, DebtInput, FinanceData, FinanceKind,
    PlanInput, PlannedPurchase, Prepaid, Transaction, TransactionInput, TxKind, days_in_month,
    month_key, summarize, upcoming,
};
use rimelight_personal_core::{ApiClient, ApiError, Rates, convert_to_brl, local_date};

/// The page's Slint markup, for the shell's build script (added as `@rimelight_personal_finances`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Income categories, as the web app's `INCOME_CATEGORIES` lists them.
pub const INCOME_CATEGORIES: &[&str] = &[
    "Freelance & Consulting",
    "Investments & Dividends",
    "Rental & Property",
    "Royalties & Content",
    "SaaS & Digital Products",
    "Salary / Primary",
    "Other Income",
];

/// Bill categories, as the web app's `BILL_CATEGORIES` lists them.
pub const BILL_CATEGORIES: &[&str] = &[
    "AI & Developer Tools",
    "Entertainment & Media",
    "Health & Fitness",
    "Housing & Utilities",
    "Infrastructure / Hosting",
    "Insurance",
    "Software / Cloud",
    "Other",
];

/// Expense categories, as the web app's `EXPENSE_CATEGORIES` lists them.
pub const EXPENSE_CATEGORIES: &[&str] = &[
    "Housing & Rent",
    "Food & Groceries",
    "Dining Out & Delivery",
    "Transport & Fuel",
    "Health & Medical",
    "Education & Courses",
    "Entertainment & Leisure",
    "Shopping & Gear",
    "Software & Subscriptions",
    "Debt & Interest Payments",
    "Investments & Savings",
    "Gifts & Donations",
    "Other Expense",
];

/// Currencies the page offers. BRL is the base currency.
pub const CURRENCIES: &[&str] = &["BRL", "USD", "CAD", "GBP", "EUR", "RON"];

type Done = Box<dyn FnOnce(Result<(), ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait FinanceApi: Send + Sync {
    fn load(&self, on_done: Box<dyn FnOnce(Result<FinanceData, ApiError>) + Send>);
    fn rates(&self, on_done: Box<dyn FnOnce(Option<Rates>) + Send>);
    fn save_account(&self, id: Option<&str>, input: &AccountInput, on_done: Done);
    fn save_transaction(&self, id: Option<&str>, input: &TransactionInput, on_done: Done);
    fn save_bill(&self, id: Option<&str>, input: &BillInput, on_done: Done);
    fn set_bill_active(&self, id: &str, is_active: bool, on_done: Done);
    fn create_budget(&self, category: &str, monthly_limit: f64, currency: &str, on_done: Done);
    fn save_budget_limit(&self, id: &str, monthly_limit: f64, on_done: Done);
    fn create_debt(&self, input: &DebtInput, on_done: Done);
    fn set_debt_paid(&self, id: &str, is_paid: bool, on_done: Done);
    fn record_debt_payment(
        &self,
        debt: &Debt,
        amount: f64,
        account_id: Option<String>,
        date: &str,
        on_done: Done,
    );
    fn save_plan(&self, id: Option<&str>, input: &PlanInput, on_done: Done);
    fn set_plan_purchased(&self, id: &str, is_purchased: bool, on_done: Done);
    fn delete(&self, kind: FinanceKind, id: &str, on_done: Done);
}

impl FinanceApi for ApiClient {
    fn load(&self, on_done: Box<dyn FnOnce(Result<FinanceData, ApiError>) + Send>) {
        ApiClient::finances(self, on_done);
    }

    fn rates(&self, on_done: Box<dyn FnOnce(Option<Rates>) + Send>) {
        rimelight_personal_core::fetch_rates(on_done);
    }

    fn save_account(&self, id: Option<&str>, input: &AccountInput, on_done: Done) {
        ApiClient::save_account(self, id, input, on_done);
    }

    fn save_transaction(&self, id: Option<&str>, input: &TransactionInput, on_done: Done) {
        ApiClient::save_transaction(self, id, input, on_done);
    }

    fn save_bill(&self, id: Option<&str>, input: &BillInput, on_done: Done) {
        ApiClient::save_bill(self, id, input, on_done);
    }

    fn set_bill_active(&self, id: &str, is_active: bool, on_done: Done) {
        ApiClient::set_bill_active(self, id, is_active, on_done);
    }

    fn create_budget(&self, category: &str, monthly_limit: f64, currency: &str, on_done: Done) {
        ApiClient::create_budget(self, category, monthly_limit, currency, on_done);
    }

    fn save_budget_limit(&self, id: &str, monthly_limit: f64, on_done: Done) {
        ApiClient::save_budget_limit(self, id, monthly_limit, on_done);
    }

    fn create_debt(&self, input: &DebtInput, on_done: Done) {
        ApiClient::create_debt(self, input, on_done);
    }

    fn set_debt_paid(&self, id: &str, is_paid: bool, on_done: Done) {
        ApiClient::set_debt_paid(self, id, is_paid, on_done);
    }

    fn record_debt_payment(
        &self,
        debt: &Debt,
        amount: f64,
        account_id: Option<String>,
        date: &str,
        on_done: Done,
    ) {
        ApiClient::record_debt_payment(self, debt, amount, account_id, date, on_done);
    }

    fn save_plan(&self, id: Option<&str>, input: &PlanInput, on_done: Done) {
        ApiClient::save_plan(self, id, input, on_done);
    }

    fn set_plan_purchased(&self, id: &str, is_purchased: bool, on_done: Done) {
        ApiClient::set_plan_purchased(self, id, is_purchased, on_done);
    }

    fn delete(&self, kind: FinanceKind, id: &str, on_done: Done) {
        ApiClient::delete_finance(self, kind, id, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn FinanceApi> {
    Arc::new(ApiClient::new(api_base))
}

// ---------------------------------------------------------------------------
// Tabs and forms
// ---------------------------------------------------------------------------

/// The page's tabs. Each is a sub-route of `/finances` on the web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FinanceTab {
    #[default]
    Overview,
    Accounts,
    Transactions,
    Incomes,
    Bills,
    Budgets,
    Debts,
    Wishlist,
}

impl FinanceTab {
    pub const ALL: [FinanceTab; 8] = [
        Self::Overview,
        Self::Accounts,
        Self::Transactions,
        Self::Incomes,
        Self::Bills,
        Self::Budgets,
        Self::Debts,
        Self::Wishlist,
    ];

    /// The key the markup and the glue use, which is also the route segment after `/finances`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Accounts => "accounts",
            Self::Transactions => "transactions",
            Self::Incomes => "incomes",
            Self::Bills => "bills",
            Self::Budgets => "budgets",
            Self::Debts => "debts",
            Self::Wishlist => "wishlist",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Accounts => "Accounts",
            Self::Transactions => "Transactions",
            Self::Incomes => "Incomes",
            Self::Bills => "Bills",
            Self::Budgets => "Budgets",
            Self::Debts => "Debts",
            Self::Wishlist => "Wishlist",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormKind {
    Account,
    Income,
    Expense,
    Bill,
    Budget,
    BudgetLimit,
    Debt,
    DebtPayment,
    Plan,
}

/// One form field: key, label, placeholder, required.
type Spec = (&'static str, &'static str, &'static str, bool);

const ACCOUNT_FIELDS: &[Spec] = &[
    ("name", "Account name", "", true),
    ("accountType", "Account type", "", false),
    ("balance", "Balance", "0.00", false),
    ("currency", "Currency", "", false),
    ("institution", "Institution", "", false),
    ("agency", "Agency", "", false),
    ("accountNumber", "Account number", "", false),
    ("paymentKey", "PIX key", "", false),
    ("creditLimit", "Credit limit", "", false),
    ("notes", "Notes", "", false),
];

const INCOME_FIELDS: &[Spec] = &[
    ("description", "Source", "", true),
    ("amount", "Amount", "0.00", true),
    ("currency", "Currency", "", false),
    ("category", "Category", "", false),
    ("accountId", "Account", "", false),
    ("date", "Date", "YYYY-MM-DD", true),
    ("prepaidMonths", "Prepaid months (optional)", "", false),
];

const EXPENSE_FIELDS: &[Spec] = &[
    ("description", "Description", "", true),
    ("amount", "Amount", "0.00", true),
    ("currency", "Currency", "", false),
    ("category", "Category", "", false),
    ("accountId", "Account", "", false),
    ("date", "Date", "YYYY-MM-DD", true),
];

const BILL_FIELDS: &[Spec] = &[
    ("name", "Name", "", true),
    ("cost", "Cost", "0.00", true),
    ("currency", "Currency", "", false),
    ("billingCycle", "Billing cycle", "", false),
    ("category", "Category", "", false),
    ("renewalDate", "Renewal date", "YYYY-MM-DD", false),
    ("paymentMethod", "Payment method", "", false),
    ("notes", "Notes", "", false),
];

const BUDGET_FIELDS: &[Spec] = &[
    ("category", "Category", "", true),
    ("monthlyLimit", "Monthly limit", "0.00", true),
    ("currency", "Currency", "", false),
];

const BUDGET_LIMIT_FIELDS: &[Spec] = &[("monthlyLimit", "Monthly limit", "0.00", true)];

const DEBT_FIELDS: &[Spec] = &[
    ("direction", "Direction", "", false),
    ("debtType", "Liability type", "", false),
    ("name", "Name", "", true),
    ("lender", "Lender or person", "", false),
    ("originalAmount", "Original amount", "0.00", false),
    ("remainingAmount", "Remaining balance", "0.00", true),
    ("currency", "Currency", "", false),
    ("interestRate", "Interest rate % (yearly)", "", false),
    ("installmentAmount", "Installment amount", "", false),
    ("totalInstallments", "Total installments", "", false),
    ("remainingInstallments", "Remaining installments", "", false),
    ("dueDateDay", "Due day (1 to 31)", "", false),
    ("endDate", "End date", "YYYY-MM-DD", false),
    ("linkedAccountId", "Linked account", "", false),
    ("notes", "Notes", "", false),
];

const DEBT_PAYMENT_FIELDS: &[Spec] = &[
    ("amount", "Amount", "0.00", true),
    ("accountId", "Account", "", false),
    ("date", "Date", "YYYY-MM-DD", true),
];

const PLAN_FIELDS: &[Spec] = &[
    ("title", "Title", "", true),
    ("estimatedCost", "Estimated cost", "0.00", false),
    ("quantity", "Quantity", "1", false),
    ("currency", "Currency", "", false),
    ("category", "Category", "", false),
    ("priority", "Priority", "", false),
    ("targetDate", "Target date", "YYYY-MM-DD", false),
    ("link", "Store link", "", false),
    ("notes", "Notes", "", false),
];

impl FormKind {
    /// The keys the markup sends with `new` chips.
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "account" => Self::Account,
            "income" => Self::Income,
            "expense" => Self::Expense,
            "bill" => Self::Bill,
            "budget" => Self::Budget,
            "debt" => Self::Debt,
            "plan" => Self::Plan,
            _ => return None,
        })
    }

    fn spec(self) -> &'static [Spec] {
        match self {
            Self::Account => ACCOUNT_FIELDS,
            Self::Income => INCOME_FIELDS,
            Self::Expense => EXPENSE_FIELDS,
            Self::Bill => BILL_FIELDS,
            Self::Budget => BUDGET_FIELDS,
            Self::BudgetLimit => BUDGET_LIMIT_FIELDS,
            Self::Debt => DEBT_FIELDS,
            Self::DebtPayment => DEBT_PAYMENT_FIELDS,
            Self::Plan => PLAN_FIELDS,
        }
    }

    fn title(self, editing: bool) -> &'static str {
        match (self, editing) {
            (Self::Account, false) => "New Account",
            (Self::Account, true) => "Edit Account",
            (Self::Income, false) => "Log Income",
            (Self::Income, true) => "Edit Income",
            (Self::Expense, false) => "Log Expense",
            (Self::Expense, true) => "Edit Expense",
            (Self::Bill, false) => "New Recurring Bill",
            (Self::Bill, true) => "Edit Recurring Bill",
            (Self::Budget, _) => "New Budget Target",
            (Self::BudgetLimit, _) => "Edit Monthly Limit",
            (Self::Debt, _) => "Add Loan / Debt",
            (Self::DebtPayment, _) => "Log Payment",
            (Self::Plan, false) => "Add Wishlist Item",
            (Self::Plan, true) => "Edit Wishlist Item",
        }
    }

    fn submit_label(self) -> &'static str {
        match self {
            Self::BudgetLimit => "Save Limit",
            Self::DebtPayment => "Record",
            _ => "Save",
        }
    }
}

/// The value a field starts with. Dates start at `today`.
fn default_value(key: &str, today: &str) -> String {
    match key {
        "accountType" => "checking",
        "currency" => "BRL",
        "billingCycle" => "monthly",
        "direction" => "borrowed",
        "debtType" => "loan",
        "priority" => "medium",
        "quantity" => "1",
        "date" => return today.to_owned(),
        _ => "",
    }
    .to_owned()
}

/// The choices a field offers, as `(value, label)`. Empty for free-text fields.
fn choices(kind: FormKind, key: &str, data: &FinanceData) -> Vec<(String, String)> {
    let plain = |items: &[&str]| -> Vec<(String, String)> {
        items
            .iter()
            .map(|item| ((*item).to_owned(), (*item).to_owned()))
            .collect()
    };
    match key {
        "accountType" => plain(&["checking", "savings", "credit", "investment", "cash"]),
        "currency" => plain(CURRENCIES),
        "billingCycle" => plain(&["weekly", "monthly", "yearly"]),
        "priority" => plain(&["low", "medium", "high", "essential"]),
        "direction" => vec![
            ("borrowed".to_owned(), "I owe".to_owned()),
            ("lent".to_owned(), "Lent to others".to_owned()),
        ],
        "debtType" => vec![
            ("loan".to_owned(), "Loan".to_owned()),
            ("financing".to_owned(), "Financing".to_owned()),
            ("installment".to_owned(), "Installment".to_owned()),
            ("credit_card".to_owned(), "Credit card debt".to_owned()),
            ("personal_debt".to_owned(), "Personal debt (IOU)".to_owned()),
            ("other".to_owned(), "Other".to_owned()),
        ],
        "accountId" | "linkedAccountId" => {
            std::iter::once((String::new(), "No account (Cash / General)".to_owned()))
                .chain(data.accounts.iter().map(|a| (a.id.clone(), a.name.clone())))
                .collect()
        }
        "category" => match kind {
            FormKind::Income => plain(INCOME_CATEGORIES),
            FormKind::Bill => plain(BILL_CATEGORIES),
            FormKind::Expense | FormKind::Budget => expense_categories(data)
                .into_iter()
                .map(|c| (c.clone(), c))
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn expense_categories(data: &FinanceData) -> Vec<String> {
    let mut out: Vec<String> = EXPENSE_CATEGORIES.iter().map(|c| (*c).to_owned()).collect();
    let mut extra: Vec<String> = data
        .budgets
        .iter()
        .map(|b| b.category.clone())
        .chain(
            data.transactions
                .iter()
                .filter(|t| !t.is_income())
                .map(|t| t.category.clone()),
        )
        .filter(|c| !c.is_empty() && !out.contains(c))
        .collect();
    extra.sort();
    extra.dedup();
    out.extend(extra);
    out
}

/// An open form: the fields as text, the row being edited (if any), and the message from the last refusal.
#[derive(Debug, Clone, PartialEq)]
struct Form {
    kind: FormKind,
    editing: Option<String>,
    fields: Vec<(String, String)>,
    error: Option<String>,
    /// The debt a payment is recorded against (payment form only).
    debt: Option<Debt>,
}

impl Form {
    fn new(kind: FormKind, editing: Option<String>, today: &str) -> Self {
        Self {
            kind,
            editing,
            fields: kind
                .spec()
                .iter()
                .map(|(key, _, _, _)| ((*key).to_owned(), default_value(key, today)))
                .collect(),
            error: None,
            debt: None,
        }
    }

    fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map_or("", |(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: impl Into<String>) {
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| k.as_str() == key) {
            slot.1 = value.into();
        }
    }
}

// ---------------------------------------------------------------------------
// Writes: a form turned into one API call
// ---------------------------------------------------------------------------

enum Write {
    Account(Option<String>, AccountInput),
    Transaction(Option<String>, TransactionInput),
    Bill(Option<String>, BillInput),
    Budget {
        category: String,
        limit: f64,
        currency: String,
    },
    BudgetLimit(String, f64),
    Debt(DebtInput),
    DebtPayment {
        debt: Debt,
        amount: f64,
        account_id: Option<String>,
        date: String,
    },
    Plan(Option<String>, PlanInput),
}

/// A row action that is a single call.
enum Op {
    SetBillActive(String, bool),
    SetDebtPaid(String, bool),
    SetPlanPurchased(String, bool),
    Delete(FinanceKind, String),
}

fn label_of(kind: FormKind, key: &str) -> &'static str {
    kind.spec()
        .iter()
        .find(|s| s.0 == key)
        .map_or("This field", |s| s.1)
}

fn text(form: &Form, key: &str) -> String {
    form.get(key).trim().to_owned()
}

fn opt(form: &Form, key: &str) -> Option<String> {
    let value = text(form, key);
    (!value.is_empty()).then_some(value)
}

fn num(form: &Form, key: &str) -> Result<Option<f64>, String> {
    let raw = text(form, key);
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<f64>()
        .map(Some)
        .map_err(|_| format!("{} must be a number.", label_of(form.kind, key)))
}

fn money_field(form: &Form, key: &str) -> Result<f64, String> {
    Ok(num(form, key)?.unwrap_or(0.0))
}

fn whole(form: &Form, key: &str) -> Result<Option<u32>, String> {
    let raw = text(form, key);
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<u32>()
        .map(Some)
        .map_err(|_| format!("{} must be a whole number.", label_of(form.kind, key)))
}

fn build_write(form: &Form) -> Result<Write, String> {
    for (key, label, _, required) in form.kind.spec() {
        if *required && form.get(key).trim().is_empty() {
            return Err(format!("{label} is required."));
        }
    }
    let editing = form.editing.clone();
    Ok(match form.kind {
        FormKind::Account => Write::Account(
            editing,
            AccountInput {
                name: text(form, "name"),
                account_type: text(form, "accountType"),
                balance: money_field(form, "balance")?,
                currency: text(form, "currency"),
                institution: text(form, "institution"),
                agency: text(form, "agency"),
                account_number: text(form, "accountNumber"),
                payment_key: text(form, "paymentKey"),
                credit_limit: num(form, "creditLimit")?,
                notes: text(form, "notes"),
            },
        ),
        FormKind::Income | FormKind::Expense => {
            let kind = if form.kind == FormKind::Income {
                TxKind::Income
            } else {
                TxKind::Expense
            };
            let date = text(form, "date");
            if ymd(&date).is_none() {
                return Err("Date must be YYYY-MM-DD.".to_owned());
            }
            let prepaid = match whole(form, "prepaidMonths")? {
                Some(months) if months > 0 && kind == TxKind::Income && editing.is_none() => {
                    let total = money_field(form, "amount")?;
                    let expires_at = add_months(&date, months).ok_or("Date must be YYYY-MM-DD.")?;
                    Some(Prepaid {
                        months,
                        recognized_monthly: total / f64::from(months),
                        expires_at,
                    })
                }
                _ => None,
            };
            Write::Transaction(
                editing,
                TransactionInput {
                    kind,
                    description: text(form, "description"),
                    amount: money_field(form, "amount")?,
                    currency: text(form, "currency"),
                    category: text(form, "category"),
                    account_id: opt(form, "accountId"),
                    date,
                    prepaid,
                },
            )
        }
        FormKind::Bill => Write::Bill(
            editing,
            BillInput {
                name: text(form, "name"),
                cost: money_field(form, "cost")?,
                currency: text(form, "currency"),
                billing_cycle: text(form, "billingCycle"),
                category: text(form, "category"),
                renewal_date: text(form, "renewalDate"),
                payment_method: text(form, "paymentMethod"),
                notes: text(form, "notes"),
            },
        ),
        FormKind::Budget => Write::Budget {
            category: text(form, "category"),
            limit: money_field(form, "monthlyLimit")?,
            currency: text(form, "currency"),
        },
        FormKind::BudgetLimit => {
            let Some(id) = editing else {
                return Err("No budget selected.".to_owned());
            };
            Write::BudgetLimit(id, money_field(form, "monthlyLimit")?)
        }
        FormKind::Debt => {
            let due_date_day = whole(form, "dueDateDay")?;
            if let Some(day) = due_date_day
                && !(1..=31).contains(&day)
            {
                return Err("Due day must be between 1 and 31.".to_owned());
            }
            Write::Debt(DebtInput {
                lent: text(form, "direction") == "lent",
                debt_type: text(form, "debtType"),
                name: text(form, "name"),
                lender: text(form, "lender"),
                original_amount: money_field(form, "originalAmount")?,
                remaining_amount: money_field(form, "remainingAmount")?,
                currency: text(form, "currency"),
                interest_rate: num(form, "interestRate")?,
                installment_amount: num(form, "installmentAmount")?,
                total_installments: whole(form, "totalInstallments")?,
                remaining_installments: whole(form, "remainingInstallments")?,
                due_date_day,
                end_date: text(form, "endDate"),
                linked_account_id: opt(form, "linkedAccountId"),
                notes: text(form, "notes"),
            })
        }
        FormKind::DebtPayment => {
            let Some(debt) = form.debt.clone() else {
                return Err("No loan selected.".to_owned());
            };
            let date = text(form, "date");
            if ymd(&date).is_none() {
                return Err("Date must be YYYY-MM-DD.".to_owned());
            }
            let amount = money_field(form, "amount")?;
            if amount <= 0.0 {
                return Err("Amount must be greater than zero.".to_owned());
            }
            Write::DebtPayment {
                debt,
                amount,
                account_id: opt(form, "accountId"),
                date,
            }
        }
        FormKind::Plan => Write::Plan(
            editing,
            PlanInput {
                title: text(form, "title"),
                estimated_cost: num(form, "estimatedCost")?,
                quantity: num(form, "quantity")?.unwrap_or(1.0),
                currency: text(form, "currency"),
                category: text(form, "category"),
                priority: text(form, "priority"),
                target_date: text(form, "targetDate"),
                link: text(form, "link"),
                notes: text(form, "notes"),
            },
        ),
    })
}

// ---------------------------------------------------------------------------
// Dates and money
// ---------------------------------------------------------------------------

/// `(year, month, day)` of a `YYYY-MM-DD` date (anything after the day is ignored).
fn ymd(value: &str) -> Option<(i64, i64, i64)> {
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<i64>().ok()?;
    let day = value.get(8..10)?.parse::<i64>().ok()?;
    let shaped = value.get(4..5) == Some("-") && value.get(7..8) == Some("-");
    (shaped && (1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

/// Adds whole months to a date, keeping the day or the last day of a shorter month.
fn add_months(date: &str, months: u32) -> Option<String> {
    let (year, month, day) = ymd(date)?;
    let total = year * 12 + (month - 1) + i64::from(months);
    let (next_year, next_month) = (total / 12, total % 12 + 1);
    let next_day = day.min(days_in_month(next_year, next_month));
    Some(format!("{next_year:04}-{next_month:02}-{next_day:02}"))
}

/// The `YYYY-MM-DD` part of a stored date, which may carry a time.
fn day10(value: &str) -> &str {
    value.get(..10).unwrap_or(value)
}

fn today_string() -> String {
    let day = local_date::today();
    format!("{:04}-{:02}-{:02}", day.year, day.month, day.day)
}

/// Groups thousands with commas and formats the amount with two decimals, prefixed by its currency.
pub fn fmt_money(amount: f64, currency: &str) -> String {
    let code = if currency.is_empty() { "BRL" } else { currency };
    let sign = if amount < 0.0 { "-" } else { "" };
    let cents = (amount.abs() * 100.0).round() as i64;
    let digits = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{code} {sign}{grouped}.{:02}", cents % 100)
}

fn brl(amount: f64, currency: &str, rates: Option<&Rates>) -> f64 {
    convert_to_brl(amount, currency, rates).unwrap_or(amount)
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .copied()
        .filter(|p| !p.is_empty())
        .collect::<Vec<&str>>()
        .join(" · ")
}

fn debt_type_label(code: &str) -> String {
    match code {
        "loan" => "Loan".to_owned(),
        "financing" => "Financing".to_owned(),
        "installment" => "Installment".to_owned(),
        "credit_card" => "Credit card debt".to_owned(),
        "personal_debt" => "Personal debt (IOU)".to_owned(),
        "loan_given" => "Loan given".to_owned(),
        "other" => "Other".to_owned(),
        other => other.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatView {
    pub label: String,
    pub value: String,
}

/// One row. `primary_label` and `secondary_label` name the row's two actions; an empty label hides the button.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowView {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub value: String,
    pub note: String,
    pub primary_label: String,
    pub secondary_label: String,
    pub can_edit: bool,
    pub can_delete: bool,
    /// Text the row's copy button puts on the clipboard (an account's PIX key). Empty hides the button.
    pub copy_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionView {
    pub title: String,
    pub empty: String,
    pub rows: Vec<RowView>,
}

/// A chip button. `group` says what a click means: `kind`, `account` or `category` filter the transactions;
/// `new` opens the form whose key is `value`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChipView {
    pub group: String,
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChoiceView {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldView {
    pub key: String,
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub required: bool,
    /// `text`, `select` (the quick choices in `choices`; the shared Select shows them) or `date` (the shared DatePicker,
    /// which shows `calendar`).
    pub kind: String,
    pub choices: Vec<ChoiceView>,
    pub calendar: Vec<CalendarCellView>,
    pub calendar_title: String,
    pub weekday_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormView {
    pub title: String,
    pub submit_label: String,
    pub error: Option<String>,
    pub fields: Vec<FieldView>,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FinancesView {
    pub tab: FinanceTab,
    pub loading: bool,
    pub error: Option<String>,
    /// True once the rates have been asked for and none came back: foreign totals then show raw amounts.
    pub rates_missing: bool,
    pub search: String,
    pub stats: Vec<StatView>,
    pub chips: Vec<ChipView>,
    pub sections: Vec<SectionView>,
    pub form: Option<FormView>,
}

#[derive(Default)]
struct State {
    data: FinanceData,
    rates: Option<Rates>,
    rates_checked: bool,
    /// Calls still running for the last refresh (the data and the rates).
    pending: u8,
    error: Option<String>,
    tab: FinanceTab,
    today: String,
    search: String,
    /// `all`, `income` or `expense`.
    kind: String,
    /// The account id to filter by. Empty for all accounts.
    account: String,
    /// The category to filter by. Empty for all categories.
    category: String,
    /// The month each date picker shows, once the user has moved it. Keyed by field key.
    date_nav: BTreeMap<String, (i64, i64)>,
    form: Option<Form>,
}

fn build_view(s: &State) -> FinancesView {
    let rates = s.rates.as_ref();
    let today = s.today.as_str();
    let month = month_key(today).to_owned();
    let summary = summarize(&s.data, rates, today);
    let mut stats = Vec::new();
    let mut chips: Vec<ChipView>;
    let mut sections = Vec::new();
    let brl_text = |amount: f64| fmt_money(amount, "BRL");

    match s.tab {
        FinanceTab::Overview => {
            stats = vec![
                stat("Net worth", brl_text(summary.net_worth)),
                stat("Total balance", brl_text(summary.total_balance)),
                stat("Money lent to others", brl_text(summary.total_lent)),
                stat("Money owed", brl_text(summary.total_borrowed)),
                stat("Income this month", brl_text(summary.monthly_income)),
                stat(
                    "Monthly net margin",
                    brl_text(
                        summary.monthly_income
                            - summary.monthly_burn
                            - summary.monthly_debt_payment,
                    ),
                ),
                stat("Monthly bill burn", brl_text(summary.monthly_burn)),
                stat(
                    "Monthly debt instalments",
                    brl_text(summary.monthly_debt_payment),
                ),
            ];
            sections.push(section(
                "Upcoming (next 30 days)",
                "Nothing due in the next 30 days.",
                upcoming(&s.data, today)
                    .into_iter()
                    .map(|u| RowView {
                        title: u.name,
                        subtitle: u.category,
                        value: fmt_money(u.amount, &u.currency),
                        note: match u.days {
                            0 => "due today".to_owned(),
                            1 => "due tomorrow".to_owned(),
                            days => format!("in {days} days"),
                        },
                        ..Default::default()
                    })
                    .collect(),
            ));
            sections.push(section(
                "Budget pacing",
                "Create a budget to track its pace.",
                budget_rows(&s.data, &month, today),
            ));
            sections.push(section(
                "Spend this month by category",
                "No spending this month.",
                category_rows(&s.data, rates, &month),
            ));
            let mut recent: Vec<&Transaction> = s.data.transactions.iter().collect();
            recent.sort_by(|a, b| b.date.cmp(&a.date));
            sections.push(section(
                "Recent transactions",
                "No transactions yet.",
                recent
                    .into_iter()
                    .take(8)
                    .map(|t| tx_row(t, &s.data.accounts))
                    .collect(),
            ));
            chips = new_chips(s.tab);
        }
        FinanceTab::Accounts => {
            stats = vec![stat("Total balance", brl_text(summary.total_balance))];
            sections.push(section(
                "Accounts",
                "Add Your First Account",
                s.data.accounts.iter().map(account_row).collect(),
            ));
            chips = new_chips(s.tab);
        }
        FinanceTab::Transactions => {
            let txs = filtered_transactions(s);
            stats = vec![stat("Records", txs.len().to_string())];
            sections.push(section(
                "Transactions",
                "No transactions matched your active filters or search criteria.",
                txs.into_iter()
                    .map(|t| tx_row(t, &s.data.accounts))
                    .collect(),
            ));
            chips = filter_chips(s);
            chips.extend(new_chips(s.tab));
        }
        FinanceTab::Incomes => {
            stats = vec![stat(
                "Recognised this month",
                brl_text(summary.monthly_income),
            )];
            let mut incomes: Vec<&Transaction> = s
                .data
                .transactions
                .iter()
                .filter(|t| t.is_income())
                .collect();
            incomes.sort_by(|a, b| b.date.cmp(&a.date));
            sections.push(section(
                "Incomes",
                "Log First Income",
                incomes
                    .into_iter()
                    .map(|t| tx_row(t, &s.data.accounts))
                    .collect(),
            ));
            chips = new_chips(s.tab);
        }
        FinanceTab::Bills => {
            stats = vec![stat(
                "Monthly subscription burn",
                brl_text(summary.monthly_burn),
            )];
            let (active, paused): (Vec<&Bill>, Vec<&Bill>) =
                s.data.bills.iter().partition(|b| b.is_active);
            sections.push(section(
                "Active",
                "Add First Bill",
                active.iter().map(bill_row).collect(),
            ));
            sections.push(section(
                "Paused",
                "No paused bills.",
                paused.iter().map(bill_row).collect(),
            ));
            chips = new_chips(s.tab);
        }
        FinanceTab::Budgets => {
            sections.push(section(
                "Budgets",
                "Create First Budget",
                budget_rows(&s.data, &month, today),
            ));
            chips = new_chips(s.tab);
        }
        FinanceTab::Debts => {
            stats = vec![
                stat(
                    "Monthly instalments",
                    brl_text(summary.monthly_debt_payment),
                ),
                stat(
                    "Net loan position",
                    brl_text(summary.total_lent - summary.total_borrowed),
                ),
            ];
            let open: Vec<&Debt> = s.data.debts.iter().filter(|d| !d.is_paid).collect();
            let receivables: Vec<RowView> = open
                .iter()
                .filter(|d| d.is_lent())
                .map(|d| debt_row(d))
                .collect();
            let liabilities: Vec<RowView> = open
                .iter()
                .filter(|d| !d.is_lent())
                .map(|d| debt_row(d))
                .collect();
            let settled: Vec<RowView> = s
                .data
                .debts
                .iter()
                .filter(|d| d.is_paid)
                .map(debt_row)
                .collect();
            sections.push(section(
                "Money lent to others (receivables)",
                "No money lent out.",
                receivables,
            ));
            sections.push(section(
                "Liabilities and money I owe",
                "No liabilities.",
                liabilities,
            ));
            sections.push(section("Settled History", "Nothing settled yet.", settled));
            chips = new_chips(s.tab);
        }
        FinanceTab::Wishlist => {
            stats = vec![
                stat("Total Wishlist Cost", brl_text(summary.total_wishlist)),
                stat(
                    "Liquid Cash Coverage",
                    format!(
                        "{:.0}%",
                        if summary.total_wishlist > 0.0 {
                            (summary.total_balance / summary.total_wishlist * 100.0)
                                .round()
                                .min(100.0)
                        } else {
                            100.0
                        }
                    ),
                ),
            ];
            let (open, bought): (Vec<&PlannedPurchase>, Vec<&PlannedPurchase>) = s
                .data
                .planned_purchases
                .iter()
                .partition(|p| !p.is_purchased);
            sections.push(section(
                "Wishlist",
                "Add Wishlist Item",
                open.iter().map(plan_row).collect(),
            ));
            sections.push(section(
                "Bought",
                "Nothing bought from the wishlist yet.",
                bought.iter().map(plan_row).collect(),
            ));
            chips = new_chips(s.tab);
        }
    }

    FinancesView {
        tab: s.tab,
        loading: s.pending > 0,
        error: s.error.clone(),
        rates_missing: s.rates_checked && s.rates.is_none(),
        search: s.search.clone(),
        stats,
        chips,
        sections,
        form: form_view(s),
    }
}

fn stat(label: &str, value: String) -> StatView {
    StatView {
        label: label.to_owned(),
        value,
    }
}

fn section(title: &str, empty: &str, rows: Vec<RowView>) -> SectionView {
    SectionView {
        title: title.to_owned(),
        empty: empty.to_owned(),
        rows,
    }
}

fn chip(group: &str, value: &str, label: &str, selected: bool) -> ChipView {
    ChipView {
        group: group.to_owned(),
        value: value.to_owned(),
        label: label.to_owned(),
        selected,
    }
}

/// The "new" buttons of a tab. Overview has the four quick buttons; the others have one.
fn new_chips(tab: FinanceTab) -> Vec<ChipView> {
    let pairs: &[(&str, &str)] = match tab {
        FinanceTab::Overview => &[
            ("income", "Log Income"),
            ("expense", "Log Expense"),
            ("bill", "Add Recurring Bill"),
            ("debt", "Add Loan / Debt"),
        ],
        FinanceTab::Accounts => &[("account", "New Account")],
        FinanceTab::Transactions => &[("expense", "Log Expense"), ("income", "Log Income")],
        FinanceTab::Incomes => &[("income", "Log Income")],
        FinanceTab::Bills => &[("bill", "New Recurring Bill")],
        FinanceTab::Budgets => &[("budget", "New Budget Target")],
        FinanceTab::Debts => &[("debt", "Add Loan / Debt")],
        FinanceTab::Wishlist => &[("plan", "Add Wishlist Item")],
    };
    pairs
        .iter()
        .map(|(value, label)| chip("new", value, label, false))
        .collect()
}

fn filter_chips(s: &State) -> Vec<ChipView> {
    let mut out = Vec::new();
    for (value, label) in [
        ("all", "All"),
        ("income", "Incomes only"),
        ("expense", "Expenses only"),
    ] {
        out.push(chip("kind", value, label, s.kind == value));
    }
    out.push(chip("account", "", "All accounts", s.account.is_empty()));
    for account in &s.data.accounts {
        out.push(chip(
            "account",
            &account.id,
            &account.name,
            s.account == account.id,
        ));
    }
    out.push(chip(
        "category",
        "",
        "All categories",
        s.category.is_empty(),
    ));
    for category in distinct_categories(&s.data) {
        out.push(chip(
            "category",
            &category,
            &category,
            s.category == category,
        ));
    }
    out
}

fn distinct_categories(data: &FinanceData) -> Vec<String> {
    let mut categories: Vec<String> = data
        .transactions
        .iter()
        .map(|t| t.category.clone())
        .filter(|c| !c.is_empty())
        .collect();
    categories.sort();
    categories.dedup();
    categories
}

fn category_rows(data: &FinanceData, rates: Option<&Rates>, month: &str) -> Vec<RowView> {
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    for t in data
        .transactions
        .iter()
        .filter(|t| !t.is_income() && month_key(&t.date) == month)
    {
        let key = if t.category.is_empty() {
            "Uncategorised".to_owned()
        } else {
            t.category.clone()
        };
        *totals.entry(key).or_insert(0.0) += brl(t.amount, &t.currency, rates);
    }
    let mut rows: Vec<(String, f64)> = totals.into_iter().collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    rows.into_iter()
        .map(|(category, total)| RowView {
            title: category,
            value: fmt_money(total, "BRL"),
            ..Default::default()
        })
        .collect()
}

fn budget_rows(data: &FinanceData, month: &str, today: &str) -> Vec<RowView> {
    let elapsed = elapsed_pct(today);
    data.budgets
        .iter()
        .map(|b: &Budget| {
            // The web's `categorySpending()[category] || b.spent || 0`: this month's expense total for the category,
            // or the stored `spent` when that total is zero.
            let category_total: f64 = data
                .transactions
                .iter()
                .filter(|t| {
                    t.kind == "expense" && month_key(&t.date) == month && t.category == b.category
                })
                .map(|t| t.amount)
                .sum();
            let spent = if category_total != 0.0 {
                category_total
            } else {
                b.spent
            };
            let (status, pct) = pace(spent, b.monthly_limit, elapsed);
            RowView {
                id: format!("budget:{}", b.id),
                title: b.category.clone(),
                subtitle: format!("{elapsed}% of the month elapsed · {pct}% of the limit used"),
                value: format!(
                    "{} of {}",
                    fmt_money(spent, &b.currency),
                    fmt_money(b.monthly_limit, &b.currency)
                ),
                note: status.to_owned(),
                can_edit: true,
                can_delete: true,
                ..Default::default()
            }
        })
        .collect()
}

fn account_row(a: &Account) -> RowView {
    let none = || String::new();
    let agency = a.agency.clone().unwrap_or_else(none);
    let number = a.account_number.clone().unwrap_or_else(none);
    let pix = a.payment_key.clone().unwrap_or_else(none);
    let limit = a
        .credit_limit
        .map(|l| format!("Limit {}", fmt_money(l, &a.currency)))
        .unwrap_or_default();
    RowView {
        id: format!("account:{}", a.id),
        title: a.name.clone(),
        subtitle: join(&[&a.account_type, a.institution.as_deref().unwrap_or("")]),
        value: fmt_money(a.balance, &a.currency),
        note: join(&[
            &if agency.is_empty() {
                String::new()
            } else {
                format!("Agency {agency}")
            },
            &if number.is_empty() {
                String::new()
            } else {
                format!("Account {number}")
            },
            &if pix.is_empty() {
                String::new()
            } else {
                format!("PIX {pix}")
            },
            &limit,
        ]),
        copy_text: a.payment_key.clone().unwrap_or_default(),
        can_edit: true,
        can_delete: true,
        ..Default::default()
    }
}

fn account_name(accounts: &[Account], id: Option<&str>) -> String {
    id.and_then(|id| accounts.iter().find(|a| a.id == id))
        .map(|a| a.name.clone())
        .unwrap_or_default()
}

fn tx_row(t: &Transaction, accounts: &[Account]) -> RowView {
    let sign = if t.is_income() { "+" } else { "-" };
    let date = day10(&t.date);
    RowView {
        id: format!("tx:{}", t.id),
        title: t
            .description
            .clone()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| t.category.clone()),
        subtitle: join(&[
            &t.category,
            date,
            &account_name(accounts, t.account_id.as_deref()),
        ]),
        value: format!("{sign}{}", fmt_money(t.amount, &t.currency)),
        note: prepaid_note(t),
        can_edit: true,
        can_delete: true,
        ..Default::default()
    }
}

fn prepaid_note(t: &Transaction) -> String {
    if !t.is_prepaid {
        return String::new();
    }
    format!(
        "Prepaid ({} mo) · recognised {}/mo · renews {}",
        t.prepaid_months.unwrap_or(0),
        fmt_money(t.recognized_monthly_amount.unwrap_or(0.0), &t.currency),
        t.prepaid_expires_at.as_deref().map_or("", day10),
    )
}

fn filtered_transactions(s: &State) -> Vec<&Transaction> {
    let needle = s.search.trim().to_lowercase();
    let mut txs: Vec<&Transaction> = s
        .data
        .transactions
        .iter()
        .filter(|t| match s.kind.as_str() {
            "income" => t.is_income(),
            "expense" => !t.is_income(),
            _ => true,
        })
        .filter(|t| s.account.is_empty() || t.account_id.as_deref() == Some(s.account.as_str()))
        .filter(|t| s.category.is_empty() || t.category == s.category)
        .filter(|t| {
            needle.is_empty()
                || t.description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&needle)
                || t.category.to_lowercase().contains(&needle)
        })
        .collect();
    txs.sort_by(|a, b| b.date.cmp(&a.date));
    txs
}

fn bill_row(b: &&Bill) -> RowView {
    let per_month = if b.billing_cycle == "monthly" {
        String::new()
    } else {
        format!("≈ {}/mo", fmt_money(b.monthly_cost(), &b.currency))
    };
    RowView {
        id: format!("bill:{}", b.id),
        title: b.name.clone(),
        subtitle: join(&[
            &b.billing_cycle,
            &b.renewal_date
                .as_deref()
                .map_or(String::new(), |d| format!("renews {}", day10(d))),
            b.payment_method.as_deref().unwrap_or(""),
        ]),
        value: fmt_money(b.cost, &b.currency),
        note: per_month,
        primary_label: if b.is_active {
            "Pause".to_owned()
        } else {
            "Resume".to_owned()
        },
        can_edit: true,
        can_delete: true,
        ..Default::default()
    }
}

fn debt_row(d: &Debt) -> RowView {
    let lent = d.is_lent();
    let paid_pct = if d.original_amount > 0.0 {
        ((d.original_amount - d.remaining_amount) / d.original_amount * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let mut parts = vec![format!(
        "{:.0}% {}",
        paid_pct,
        if lent { "collected" } else { "paid off" }
    )];
    if let (Some(left), Some(total)) = (d.remaining_installments, d.total_installments) {
        parts.push(format!("{left} of {total} instalments left"));
    }
    if let Some(day) = d.due_date_day {
        parts.push(format!("due day {day}"));
    }
    if let Some(amount) = d.installment_amount {
        parts.push(format!("{} instalment", fmt_money(amount, &d.currency)));
    }
    let subtitle = if lent {
        d.lender
            .clone()
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "Lent to others".to_owned())
    } else {
        debt_type_label(&d.debt_type)
    };
    let (primary_label, secondary_label) = if d.is_paid {
        ("Reopen".to_owned(), String::new())
    } else {
        (
            "Mark Fully Settled".to_owned(),
            if lent {
                "Record Repayment Received"
            } else {
                "Log Payment"
            }
            .to_owned(),
        )
    };
    RowView {
        id: format!("debt:{}", d.id),
        title: d.name.clone(),
        subtitle,
        value: fmt_money(d.remaining_amount, &d.currency),
        note: parts.join(" · "),
        primary_label,
        secondary_label,
        can_edit: false,
        can_delete: true,
        copy_text: String::new(),
    }
}

fn plan_row(p: &&PlannedPurchase) -> RowView {
    RowView {
        id: format!("plan:{}", p.id),
        title: p.title.clone(),
        subtitle: join(&[
            &p.category,
            &p.priority,
            day10(p.target_date.as_deref().unwrap_or("")),
        ]),
        value: fmt_money(p.item_total(), &p.currency),
        note: p.link.clone().unwrap_or_default(),
        primary_label: if p.is_purchased {
            "Move back to Wishlist".to_owned()
        } else {
            "Mark as Bought".to_owned()
        },
        can_edit: true,
        can_delete: true,
        ..Default::default()
    }
}

fn form_view(s: &State) -> Option<FormView> {
    let form = s.form.as_ref()?;
    let title = match (form.kind, &form.debt) {
        (FormKind::DebtPayment, Some(debt)) if debt.is_lent() => "Record Repayment Received",
        _ => form.kind.title(form.editing.is_some()),
    };
    let today = s.today.as_str();
    let fields = form
        .kind
        .spec()
        .iter()
        .map(|(key, label, placeholder, required)| {
            let value = form.get(key).to_owned();
            let options = choices(form.kind, key, &s.data);
            let is_date = DATE_KEYS.contains(key);
            let kind = if is_date {
                "date"
            } else if options.is_empty() {
                "text"
            } else {
                "select"
            };
            let (calendar, calendar_title) = if is_date {
                let cal = calendar_for(&value, s.date_nav.get(*key).copied(), today);
                (cal.cells, cal.title)
            } else {
                (Vec::new(), String::new())
            };
            let choices = options
                .into_iter()
                .map(|(choice, text)| ChoiceView {
                    selected: choice == value,
                    value: choice,
                    label: text,
                })
                .collect();
            FieldView {
                key: (*key).to_owned(),
                label: (*label).to_owned(),
                placeholder: (*placeholder).to_owned(),
                required: *required,
                kind: kind.to_owned(),
                choices,
                calendar,
                calendar_title,
                weekday_labels: if is_date {
                    WEEKDAYS.iter().map(|d| (*d).to_owned()).collect()
                } else {
                    Vec::new()
                },
                value,
            }
        })
        .collect();
    Some(FormView {
        title: title.to_owned(),
        submit_label: form.kind.submit_label().to_owned(),
        error: form.error.clone(),
        fields,
    })
}

fn split_row(row_id: &str) -> Option<(&str, &str)> {
    row_id.split_once(':')
}

fn kind_of(prefix: &str) -> Option<FinanceKind> {
    Some(match prefix {
        "account" => FinanceKind::Account,
        "tx" => FinanceKind::Transaction,
        "budget" => FinanceKind::Budget,
        "bill" => FinanceKind::Bill,
        "plan" => FinanceKind::Plan,
        "debt" => FinanceKind::Debt,
        _ => return None,
    })
}

/// The form for editing the row `prefix:id`, filled from the current data. `None` for rows that cannot be edited.
fn prefill(data: &FinanceData, today: &str, prefix: &str, id: &str) -> Option<Form> {
    let mut form = match prefix {
        "account" => {
            let a = data.accounts.iter().find(|a| a.id == id)?;
            let mut f = Form::new(FormKind::Account, Some(id.to_owned()), today);
            f.set("name", a.name.as_str());
            f.set("accountType", a.account_type.as_str());
            f.set("balance", a.balance.to_string());
            f.set("currency", a.currency.as_str());
            f.set("institution", a.institution.clone().unwrap_or_default());
            f.set("agency", a.agency.clone().unwrap_or_default());
            f.set(
                "accountNumber",
                a.account_number.clone().unwrap_or_default(),
            );
            f.set("paymentKey", a.payment_key.clone().unwrap_or_default());
            f.set(
                "creditLimit",
                a.credit_limit.map_or(String::new(), |v| v.to_string()),
            );
            f.set("notes", a.notes.clone().unwrap_or_default());
            f
        }
        "tx" => {
            let t = data.transactions.iter().find(|t| t.id == id)?;
            let kind = if t.is_income() {
                FormKind::Income
            } else {
                FormKind::Expense
            };
            let mut f = Form::new(kind, Some(id.to_owned()), today);
            f.set("description", t.description.clone().unwrap_or_default());
            f.set("amount", t.amount.to_string());
            f.set("currency", t.currency.as_str());
            f.set("category", t.category.as_str());
            f.set("accountId", t.account_id.clone().unwrap_or_default());
            f.set("date", day10(&t.date));
            f
        }
        "bill" => {
            let b = data.bills.iter().find(|b| b.id == id)?;
            let mut f = Form::new(FormKind::Bill, Some(id.to_owned()), today);
            f.set("name", b.name.as_str());
            f.set("cost", b.cost.to_string());
            f.set("currency", b.currency.as_str());
            f.set("billingCycle", b.billing_cycle.as_str());
            f.set("category", b.category.as_str());
            f.set("renewalDate", b.renewal_date.clone().unwrap_or_default());
            f.set(
                "paymentMethod",
                b.payment_method.clone().unwrap_or_default(),
            );
            f.set("notes", b.notes.clone().unwrap_or_default());
            f
        }
        "budget" => {
            let b = data.budgets.iter().find(|b| b.id == id)?;
            let mut f = Form::new(FormKind::BudgetLimit, Some(id.to_owned()), today);
            f.set("monthlyLimit", b.monthly_limit.to_string());
            f
        }
        "plan" => {
            let p = data.planned_purchases.iter().find(|p| p.id == id)?;
            let mut f = Form::new(FormKind::Plan, Some(id.to_owned()), today);
            f.set("title", p.title.as_str());
            f.set(
                "estimatedCost",
                p.estimated_cost.map_or(String::new(), |v| v.to_string()),
            );
            f.set(
                "quantity",
                p.quantity.map_or(String::new(), |v| v.to_string()),
            );
            f.set("currency", p.currency.as_str());
            f.set("category", p.category.as_str());
            f.set("priority", p.priority.as_str());
            f.set("targetDate", p.target_date.as_deref().map_or("", day10));
            f.set("link", p.link.clone().unwrap_or_default());
            f.set("notes", p.notes.clone().unwrap_or_default());
            f
        }
        _ => return None,
    };
    form.error = None;
    Some(form)
}

// ---------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------

struct Shared {
    api: Arc<dyn FinanceApi>,
    state: Mutex<State>,
    notify: Arc<dyn Fn(FinancesView) + Send + Sync>,
}

/// The Finances page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct FinancesController {
    shared: Arc<Shared>,
}

impl FinancesController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn FinanceApi>,
        notify: impl Fn(FinancesView) + Send + Sync + 'static,
    ) -> Self {
        let state = State {
            today: today_string(),
            kind: "all".to_owned(),
            ..State::default()
        };
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::new(state),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> FinancesView {
        build_view(&self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn change(&self, f: impl FnOnce(&mut State)) {
        let view = {
            let mut s = self.lock();
            f(&mut s);
            build_view(&s)
        };
        (self.shared.notify)(view);
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| s.error = Some(error.to_string()));
    }

    /// Loads the finances and the FX rates. The loading flag shows until both answer.
    pub fn refresh(&self) {
        self.change(|s| {
            s.pending = 2;
            s.today = today_string();
        });
        let me = self.clone();
        self.shared
            .api
            .load(Box::new(move |result: Result<FinanceData, ApiError>| {
                me.change(|s| {
                    s.pending = s.pending.saturating_sub(1);
                    match result {
                        Ok(data) => {
                            s.data = data;
                            s.error = None;
                        }
                        Err(e) => s.error = Some(e.to_string()),
                    }
                });
            }));
        let me = self.clone();
        self.shared.api.rates(Box::new(move |rates: Option<Rates>| {
            me.change(|s| {
                s.pending = s.pending.saturating_sub(1);
                s.rates = rates;
                s.rates_checked = true;
            });
        }));
    }

    /// Sets the day the page treats as today (the month, the pace and the default dates). Used by tests.
    pub fn set_today(&self, day: &str) {
        let day = day.to_owned();
        self.change(|s| s.today = day);
    }

    pub fn set_tab(&self, tab: FinanceTab) {
        self.change(|s| {
            s.tab = tab;
            s.form = None;
        });
    }

    pub fn set_search(&self, text: &str) {
        let text = text.to_owned();
        self.change(|s| s.search = text);
    }

    /// A chip click: a filter (`kind`, `account`, `category`) or a `new` button that opens a form.
    pub fn choose(&self, group: &str, value: &str) {
        let value = value.to_owned();
        match group {
            "kind" => self.change(move |s| s.kind = value),
            "account" => self.change(move |s| s.account = value),
            "category" => self.change(move |s| s.category = value),
            "new" => self.open_form(&value),
            _ => {}
        }
    }

    /// Opens a new form. `key` is one of `account`, `income`, `expense`, `bill`, `budget`, `debt` or `plan`.
    pub fn open_form(&self, key: &str) {
        let Some(kind) = FormKind::from_key(key) else {
            return;
        };
        self.change(|s| {
            let form = Form::new(kind, None, &s.today);
            s.date_nav.clear();
            s.form = Some(form);
        });
    }

    /// Sets one field of the open form. Quick picks and typed text both come through here.
    pub fn set_field(&self, key: &str, value: &str) {
        let (key, value) = (key.to_owned(), value.to_owned());
        self.change(|s| {
            if let Some(form) = s.form.as_mut() {
                form.set(&key, value);
            }
        });
    }

    /// Moves the date picker of the field `key` by `delta` months.
    pub fn date_step(&self, key: &str, delta: i64) {
        let key = key.to_owned();
        self.change(move |s| {
            let Some(form) = s.form.as_ref() else { return };
            let (year, month) =
                shown_month(form.get(&key), s.date_nav.get(&key).copied(), &s.today);
            let next = shift_month(year, month, delta);
            s.date_nav.insert(key, next);
        });
    }

    /// Sets the date field `key` to the day of the picker cell at `index`.
    pub fn pick_date(&self, key: &str, index: usize) {
        let key = key.to_owned();
        self.change(move |s| {
            let Some(form) = s.form.as_ref() else { return };
            let shown = shown_month(form.get(&key), s.date_nav.get(&key).copied(), &s.today);
            let date = cell_date(shown, index);
            if let Some(form) = s.form.as_mut() {
                form.set(&key, date);
            }
        });
    }

    pub fn cancel_form(&self) {
        self.change(|s| s.form = None);
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    /// Saves the open form. Returns `false`, and calls nothing, when a required field is blank or a value is invalid.
    pub fn submit(&self) -> bool {
        let form = {
            let s = self.lock();
            match s.form.clone() {
                Some(form) => form,
                None => return false,
            }
        };
        let write = match build_write(&form) {
            Ok(write) => write,
            Err(message) => {
                self.change(|s| {
                    if let Some(form) = s.form.as_mut() {
                        form.error = Some(message);
                    }
                });
                return false;
            }
        };
        self.perform(write);
        true
    }

    fn perform(&self, write: Write) {
        let me = self.clone();
        let done: Done = Box::new(move |result: Result<(), ApiError>| match result {
            Ok(()) => {
                me.change(|s| {
                    s.form = None;
                    s.error = None;
                });
                me.refresh();
            }
            Err(e) => me.fail(e),
        });
        let api = &*self.shared.api;
        match write {
            Write::Account(id, input) => api.save_account(id.as_deref(), &input, done),
            Write::Transaction(id, input) => api.save_transaction(id.as_deref(), &input, done),
            Write::Bill(id, input) => api.save_bill(id.as_deref(), &input, done),
            Write::Budget {
                category,
                limit,
                currency,
            } => api.create_budget(&category, limit, &currency, done),
            Write::BudgetLimit(id, limit) => api.save_budget_limit(&id, limit, done),
            Write::Debt(input) => api.create_debt(&input, done),
            Write::DebtPayment {
                debt,
                amount,
                account_id,
                date,
            } => {
                api.record_debt_payment(&debt, amount, account_id, &date, done);
            }
            Write::Plan(id, input) => api.save_plan(id.as_deref(), &input, done),
        }
    }

    fn run(&self, op: Op) {
        let me = self.clone();
        let done: Done = Box::new(move |result: Result<(), ApiError>| match result {
            Ok(()) => me.refresh(),
            Err(e) => me.fail(e),
        });
        let api = &*self.shared.api;
        match op {
            Op::SetBillActive(id, active) => api.set_bill_active(&id, active, done),
            Op::SetDebtPaid(id, paid) => api.set_debt_paid(&id, paid, done),
            Op::SetPlanPurchased(id, purchased) => api.set_plan_purchased(&id, purchased, done),
            Op::Delete(kind, id) => api.delete(kind, &id, done),
        }
    }

    /// Opens the form for the row `row_id`. Rows of accounts, transactions, bills, budgets and wishlist items.
    pub fn edit(&self, row_id: &str) {
        let Some((prefix, id)) = split_row(row_id) else {
            return;
        };
        let form = {
            let s = self.lock();
            prefill(&s.data, &s.today, prefix, id)
        };
        if let Some(form) = form {
            self.change(|s| s.form = Some(form));
        }
    }

    /// The row's primary action: pause or resume a bill, settle or reopen a debt, or mark a wishlist item bought.
    pub fn primary(&self, row_id: &str) {
        let Some((prefix, id)) = split_row(row_id) else {
            return;
        };
        let op = {
            let s = self.lock();
            match prefix {
                "bill" => s
                    .data
                    .bills
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| Op::SetBillActive(id.to_owned(), !b.is_active)),
                "debt" => s
                    .data
                    .debts
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| Op::SetDebtPaid(id.to_owned(), !d.is_paid)),
                "plan" => s
                    .data
                    .planned_purchases
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| Op::SetPlanPurchased(id.to_owned(), !p.is_purchased)),
                _ => None,
            }
        };
        if let Some(op) = op {
            self.run(op);
        }
    }

    /// The row's secondary action: for a debt, opens the payment form (log a payment, or record a repayment received).
    pub fn secondary(&self, row_id: &str) {
        let Some(("debt", id)) = split_row(row_id) else {
            return;
        };
        let id = id.to_owned();
        self.change(move |s| {
            let Some(debt) = s.data.debts.iter().find(|d| d.id == id).cloned() else {
                return;
            };
            let mut form = Form::new(FormKind::DebtPayment, Some(id.clone()), &s.today);
            form.set(
                "accountId",
                debt.linked_account_id.clone().unwrap_or_default(),
            );
            form.debt = Some(debt);
            s.form = Some(form);
        });
    }

    /// Deletes the row `row_id`, then reloads.
    pub fn delete(&self, row_id: &str) {
        let Some((prefix, id)) = split_row(row_id) else {
            return;
        };
        let Some(kind) = kind_of(prefix) else { return };
        self.run(Op::Delete(kind, id.to_owned()));
    }
}

// ---------------------------------------------------------------------------
// Date picker: the calendar grid the shared DatePicker shows (Sunday first, 42 cells)
// ---------------------------------------------------------------------------

const WEEKDAYS: &[&str] = &["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

const MONTHS: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The form fields that take a date. They open the date picker.
const DATE_KEYS: &[&str] = &["date", "renewalDate", "endDate", "targetDate"];

/// One day of the picker's 42-cell grid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarCellView {
    pub day: i32,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
}

struct Calendar {
    cells: Vec<CalendarCellView>,
    title: String,
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

/// The `(year, month, day)` of a day count from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The month `delta` months after `(year, month)`.
fn shift_month(year: i64, month: i64, delta: i64) -> (i64, i64) {
    let total = year * 12 + (month - 1) + delta;
    (total.div_euclid(12), total.rem_euclid(12) + 1)
}

/// The month a date picker opens on: the navigated month, else the value's month, else today's.
fn shown_month(value: &str, nav: Option<(i64, i64)>, today: &str) -> (i64, i64) {
    nav.or_else(|| ymd(value).map(|(y, m, _)| (y, m)))
        .or_else(|| ymd(today).map(|(y, m, _)| (y, m)))
        .unwrap_or((1970, 1))
}

/// The day count of the first cell: the Sunday on or before the 1st of the month.
fn grid_start(year: i64, month: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    first - (first + 4).rem_euclid(7)
}

fn calendar_for(value: &str, nav: Option<(i64, i64)>, today: &str) -> Calendar {
    let (year, month) = shown_month(value, nav, today);
    let start = grid_start(year, month);
    let selected = ymd(value);
    let now = ymd(today);
    let cells = (0..42)
        .map(|i| {
            let date = civil_from_days(start + i);
            CalendarCellView {
                day: date.2 as i32,
                in_month: date.0 == year && date.1 == month,
                today: now == Some(date),
                selected: selected == Some(date),
            }
        })
        .collect();
    Calendar {
        cells,
        title: format!("{} {year}", MONTHS[(month - 1) as usize]),
    }
}

/// The `YYYY-MM-DD` of the picker cell at `index`, in the grid of `shown`.
fn cell_date(shown: (i64, i64), index: usize) -> String {
    let (year, month, day) = civil_from_days(grid_start(shown.0, shown.1) + index as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The web's month-elapsed share, rounded and capped at 100.
fn elapsed_pct(today: &str) -> i64 {
    ymd(today).map_or(0, |(year, month, day)| {
        ((day as f64 / days_in_month(year, month) as f64) * 100.0)
            .round()
            .min(100.0) as i64
    })
}

/// The web's budget status. `pct` is spent over limit, rounded and capped at 150. Over the limit is `Exceeded`; a
/// `pct` above the month's elapsed share is `Burning Fast`; otherwise `Healthy`.
fn pace(spent: f64, limit: f64, elapsed: i64) -> (&'static str, i64) {
    let pct = if limit > 0.0 {
        ((spent / limit) * 100.0).round().min(150.0) as i64
    } else {
        0
    };
    let over = spent > limit;
    let fast = pct > elapsed && !over;
    let status = if over {
        "Exceeded"
    } else if fast {
        "Burning Fast"
    } else {
        "Healthy"
    };
    (status, pct)
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// Records every call and answers at once from the scripted data. `fail` makes each write answer with an error.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        data: FinanceData,
        rates: Option<Rates>,
        fail: bool,
    }

    impl Fake {
        fn record(&self, call: &str) {
            self.calls.lock().expect("calls lock").push(call.to_owned());
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn answer(&self, on_done: Done) {
            on_done(if self.fail {
                Err(ApiError::Transport("offline".to_owned()))
            } else {
                Ok(())
            });
        }
    }

    impl FinanceApi for Fake {
        fn load(&self, on_done: Box<dyn FnOnce(Result<FinanceData, ApiError>) + Send>) {
            self.record("load");
            on_done(Ok(self.data.clone()));
        }

        fn rates(&self, on_done: Box<dyn FnOnce(Option<Rates>) + Send>) {
            self.record("rates");
            on_done(self.rates.clone());
        }

        fn save_account(&self, id: Option<&str>, _: &AccountInput, on_done: Done) {
            self.record(&format!("save_account:{}", id.unwrap_or("new")));
            self.answer(on_done);
        }

        fn save_transaction(&self, id: Option<&str>, _: &TransactionInput, on_done: Done) {
            self.record(&format!("save_transaction:{}", id.unwrap_or("new")));
            self.answer(on_done);
        }

        fn save_bill(&self, id: Option<&str>, _: &BillInput, on_done: Done) {
            self.record(&format!("save_bill:{}", id.unwrap_or("new")));
            self.answer(on_done);
        }

        fn set_bill_active(&self, id: &str, is_active: bool, on_done: Done) {
            self.record(&format!("set_bill_active:{id}:{is_active}"));
            self.answer(on_done);
        }

        fn create_budget(&self, category: &str, monthly_limit: f64, _: &str, on_done: Done) {
            self.record(&format!("create_budget:{category}:{monthly_limit}"));
            self.answer(on_done);
        }

        fn save_budget_limit(&self, id: &str, monthly_limit: f64, on_done: Done) {
            self.record(&format!("save_budget_limit:{id}:{monthly_limit}"));
            self.answer(on_done);
        }

        fn create_debt(&self, input: &DebtInput, on_done: Done) {
            self.record(&format!("create_debt:{}", input.name));
            self.answer(on_done);
        }

        fn set_debt_paid(&self, id: &str, is_paid: bool, on_done: Done) {
            self.record(&format!("set_debt_paid:{id}:{is_paid}"));
            self.answer(on_done);
        }

        fn record_debt_payment(
            &self,
            debt: &Debt,
            amount: f64,
            _: Option<String>,
            date: &str,
            on_done: Done,
        ) {
            self.record(&format!("record_debt_payment:{}:{amount}:{date}", debt.id));
            self.answer(on_done);
        }

        fn save_plan(&self, id: Option<&str>, _: &PlanInput, on_done: Done) {
            self.record(&format!("save_plan:{}", id.unwrap_or("new")));
            self.answer(on_done);
        }

        fn set_plan_purchased(&self, id: &str, is_purchased: bool, on_done: Done) {
            self.record(&format!("set_plan_purchased:{id}:{is_purchased}"));
            self.answer(on_done);
        }

        fn delete(&self, kind: FinanceKind, id: &str, on_done: Done) {
            self.record(&format!("delete:{kind:?}:{id}"));
            self.answer(on_done);
        }
    }

    fn controller(fake: Fake) -> (FinancesController, Arc<Fake>) {
        let fake = Arc::new(fake);
        let api: Arc<dyn FinanceApi> = fake.clone();
        (FinancesController::new(api, |_| {}), fake)
    }

    fn sample_data() -> FinanceData {
        FinanceData {
            accounts: vec![Account {
                id: "a1".into(),
                name: "Caixa".into(),
                account_type: "checking".into(),
                balance: 1000.0,
                currency: "BRL".into(),
                ..Default::default()
            }],
            transactions: vec![
                Transaction {
                    id: "t1".into(),
                    amount: 150.0,
                    currency: "BRL".into(),
                    kind: "expense".into(),
                    category: "Food & Groceries".into(),
                    description: Some("Mercado".into()),
                    date: "2026-03-05".into(),
                    account_id: Some("a1".into()),
                    ..Default::default()
                },
                Transaction {
                    id: "t2".into(),
                    amount: 900.0,
                    currency: "BRL".into(),
                    kind: "income".into(),
                    category: "Salary / Primary".into(),
                    description: Some("Pagamento".into()),
                    date: "2026-03-01".into(),
                    account_id: Some("a1".into()),
                    ..Default::default()
                },
            ],
            budgets: vec![Budget {
                id: "b1".into(),
                category: "Food & Groceries".into(),
                monthly_limit: 100.0,
                currency: "BRL".into(),
                ..Default::default()
            }],
            bills: vec![Bill {
                id: "bill1".into(),
                name: "Netflix".into(),
                cost: 40.0,
                currency: "BRL".into(),
                billing_cycle: "monthly".into(),
                is_active: true,
                ..Default::default()
            }],
            planned_purchases: vec![PlannedPurchase {
                id: "p1".into(),
                title: "Desk".into(),
                estimated_cost: Some(500.0),
                quantity: Some(1.0),
                currency: "BRL".into(),
                priority: "high".into(),
                ..Default::default()
            }],
            debts: vec![Debt {
                id: "d1".into(),
                name: "Car loan".into(),
                debt_type: "loan".into(),
                original_amount: 1000.0,
                remaining_amount: 600.0,
                currency: "BRL".into(),
                ..Default::default()
            }],
        }
    }

    fn fake_with(data: FinanceData) -> Fake {
        Fake {
            data,
            rates: None,
            ..Default::default()
        }
    }

    fn stat_value(view: &FinancesView, label: &str) -> Option<String> {
        view.stats
            .iter()
            .find(|s| s.label == label)
            .map(|s| s.value.clone())
    }

    #[test]
    fn refresh_loads_data_and_reports_missing_rates() {
        let (c, _) = controller(fake_with(sample_data()));
        c.refresh();
        let view = c.view();
        assert!(!view.loading);
        assert!(view.rates_missing);
        assert_eq!(
            stat_value(&view, "Total balance").as_deref(),
            Some("BRL 1,000.00")
        );
    }

    #[test]
    fn blank_required_field_is_refused_without_a_call() {
        let (c, fake) = controller(fake_with(sample_data()));
        c.open_form("account");
        assert!(!c.submit());
        let view = c.view();
        assert_eq!(
            view.form.and_then(|f| f.error).as_deref(),
            Some("Account name is required.")
        );
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn saving_an_account_posts_and_reloads() {
        let (c, fake) = controller(fake_with(sample_data()));
        c.open_form("account");
        c.set_field("name", "Nubank");
        c.set_field("balance", "250");
        assert!(c.submit());
        let calls = fake.calls();
        assert!(calls.contains(&"save_account:new".to_owned()));
        assert!(calls.contains(&"load".to_owned()));
        assert!(c.view().form.is_none());
    }

    #[test]
    fn failed_write_keeps_the_error_on_the_page() {
        let mut fake = fake_with(sample_data());
        fake.fail = true;
        let (c, _) = controller(fake);
        c.open_form("bill");
        c.set_field("name", "Spotify");
        c.set_field("cost", "20");
        assert!(c.submit());
        assert_eq!(c.view().error.as_deref(), Some("network error: offline"));
    }

    #[test]
    fn bill_primary_action_pauses_then_the_reload_follows() {
        let (c, fake) = controller(fake_with(sample_data()));
        c.refresh();
        c.set_tab(FinanceTab::Bills);
        c.primary("bill:bill1");
        assert!(
            fake.calls()
                .contains(&"set_bill_active:bill1:false".to_owned())
        );
    }

    #[test]
    fn debt_secondary_action_records_a_payment() {
        let (c, fake) = controller(fake_with(sample_data()));
        c.refresh();
        c.set_tab(FinanceTab::Debts);
        c.secondary("debt:d1");
        c.set_field("amount", "100");
        assert!(c.submit());
        let calls = fake.calls();
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with("record_debt_payment:d1:100"))
        );
    }

    #[test]
    fn transactions_filter_by_search_and_kind() {
        let (c, _) = controller(fake_with(sample_data()));
        c.set_tab(FinanceTab::Transactions);
        c.refresh();
        c.set_search("mercado");
        assert_eq!(c.view().sections[0].rows.len(), 1);
        c.set_search("");
        c.choose("kind", "income");
        let view = c.view();
        assert_eq!(view.sections[0].rows.len(), 1);
        assert_eq!(view.sections[0].rows[0].title, "Pagamento");
    }

    #[test]
    fn budget_over_its_limit_is_marked_exceeded() {
        let (c, _) = controller(fake_with(sample_data()));
        c.refresh();
        c.set_tab(FinanceTab::Budgets);
        c.set_today("2026-03-15");
        let row = &c.view().sections[0].rows[0];
        assert_eq!(row.note, "Exceeded");
        assert_eq!(row.id, "budget:b1");
    }

    #[test]
    fn delete_removes_the_row_and_reloads() {
        let (c, fake) = controller(fake_with(sample_data()));
        c.delete("tx:t1");
        assert!(fake.calls().contains(&"delete:Transaction:t1".to_owned()));
    }

    #[test]
    fn edit_prefills_the_account_form() {
        let (c, _) = controller(fake_with(sample_data()));
        c.refresh();
        c.edit("account:a1");
        let view = c.view();
        let form = view.form.expect("form open");
        assert_eq!(form.title, "Edit Account");
        let name = form
            .fields
            .iter()
            .find(|f| f.key == "name")
            .expect("name field");
        assert_eq!(name.value, "Caixa");
    }

    #[test]
    fn add_months_keeps_the_day_where_it_can() {
        assert_eq!(add_months("2026-01-31", 1).as_deref(), Some("2026-02-28"));
        assert_eq!(add_months("2026-11-10", 3).as_deref(), Some("2027-02-10"));
    }

    #[test]
    fn formats_money_with_grouping() {
        assert_eq!(fmt_money(1234567.891, "BRL"), "BRL 1,234,567.89");
        assert_eq!(fmt_money(-5.5, "USD"), "USD -5.50");
    }

    #[test]
    fn tabs_round_trip_their_keys() {
        for tab in FinanceTab::ALL {
            assert_eq!(FinanceTab::from_key(tab.key()), Some(tab));
        }
        let mut seen = BTreeMap::new();
        for tab in FinanceTab::ALL {
            seen.insert(tab.key(), tab.label());
        }
        assert_eq!(seen.len(), 8);
    }
}
