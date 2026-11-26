//! The Overview (home) page controller (web: `src/components/overview/DashboardOverview.tsx`; egui:
//! `crates/app/src/dashboard.rs`).
//!
//! Plain Rust with no Slint. The page loads the todos, finances, notes and exchange rates in parallel, derives the
//! counts and totals the web shows, and can tick a todo or add a quick task for today. Navigation links are the
//! shell's job: the markup sends `navigate-requested` with the route.
//!
//! Threading follows the Todos page: calls return at once, the lock is never held across a call, and `notify` is
//! called after the lock is released.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rimelight_personal_core::finance::{Account, Bill, FinanceData};
use rimelight_personal_core::local_date::{Day, local_day_of, today, utc_today_iso};
use rimelight_personal_core::{
    ApiClient, ApiError, NewDatedTodo, Note, Rates, Todo, convert_to_brl, fetch_rates,
};

/// The page's Slint markup, for the shell's build script (added as `@rimelight_personal_feature_overview`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// How many pending tasks the page lists (web `pendingTodos().slice(0, 6)`).
pub const TASK_PREVIEW: usize = 6;
/// How many pinned notes the page shows (web `pinnedNotes().slice(0, 3)`).
pub const NOTE_PREVIEW: usize = 3;

pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the reads the page makes and the two writes. [`ApiClient`] implements it; tests inject a fake.
pub trait OverviewApi: Send + Sync {
    fn todos(&self, on_done: Done<Vec<Todo>>);
    fn finances(&self, on_done: Done<FinanceData>);
    fn notes(&self, on_done: Done<Vec<Note>>);
    /// The exchange rates, or `None` when they cannot be fetched (the page then shows amounts unconverted).
    fn rates(&self, on_done: Done<Option<Rates>>);
    /// Creates a todo for `due_date` (`YYYY-MM-DD`) with the quick-task defaults.
    fn create_quick(&self, title: &str, due_date: &str, on_done: Done<Todo>);
    fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>);
}

impl OverviewApi for ApiClient {
    fn todos(&self, on_done: Done<Vec<Todo>>) {
        self.list(on_done);
    }

    fn finances(&self, on_done: Done<FinanceData>) {
        ApiClient::finances(self, on_done);
    }

    fn notes(&self, on_done: Done<Vec<Note>>) {
        ApiClient::notes(self, on_done);
    }

    fn rates(&self, on_done: Done<Option<Rates>>) {
        fetch_rates(move |rates| on_done(Ok(rates)));
    }

    fn create_quick(&self, title: &str, due_date: &str, on_done: Done<Todo>) {
        let todo = NewDatedTodo {
            title,
            category: "General",
            priority: "medium",
            due_date,
        };
        self.create_dated(&todo, on_done);
    }

    fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>) {
        ApiClient::set_completed(self, id, is_completed, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn OverviewApi> {
    Arc::new(ApiClient::new(api_base))
}

/// One pending task in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    /// The priority pill's tone: `urgent`, `high` or `normal`.
    pub tone: String,
}

/// One pinned note card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteCard {
    pub id: String,
    pub title: String,
    /// The note's content, or "Empty note..." when it has none.
    pub preview: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverviewView {
    /// "Sunday, November 22, 2026".
    pub date: String,
    pub pending_count: usize,
    pub due_today_count: usize,
    pub urgent_count: usize,
    /// The pending tasks shown in the list (at most [`TASK_PREVIEW`]).
    pub tasks: Vec<TaskRow>,
    /// Every todo, for the "View all (n)" link.
    pub todo_total: usize,
    /// Liquid balance in BRL, as text such as "R$ 1.234,50".
    pub net_worth: String,
    pub account_count: usize,
    /// Active bills per month in BRL, as text.
    pub monthly_bills: String,
    pub active_subscriptions: usize,
    pub notes: Vec<NoteCard>,
    /// Every note, for the "Notes (n)" link.
    pub note_total: usize,
    pub quick_task: String,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    todos: Vec<Todo>,
    finances: FinanceData,
    notes: Vec<Note>,
    rates: Option<Rates>,
    quick_task: String,
    loading: bool,
    error: Option<String>,
}

impl State {
    fn view(&self, today: Day, date: String) -> OverviewView {
        let rates = self.rates.as_ref();
        let pending: Vec<&Todo> = self.todos.iter().filter(|t| !t.is_completed).collect();
        let due_today = pending
            .iter()
            .filter(|t| is_today(t.due_date.as_deref(), today))
            .count();
        let urgent = pending
            .iter()
            .filter(|t| t.priority == "urgent" || t.priority == "high")
            .count();

        let net_worth: f64 = self
            .finances
            .accounts
            .iter()
            .map(|a| account_in_brl(a, rates))
            .sum();
        let monthly_bills: f64 = self
            .finances
            .bills
            .iter()
            .filter(|b| b.is_active)
            .map(|b| bill_monthly_in_brl(b, rates))
            .sum();

        OverviewView {
            date,
            pending_count: pending.len(),
            due_today_count: due_today,
            urgent_count: urgent,
            tasks: pending
                .iter()
                .take(TASK_PREVIEW)
                .map(|t| TaskRow {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    category: t.category.clone(),
                    priority: t.priority.clone(),
                    tone: match t.priority.as_str() {
                        "urgent" => "urgent",
                        "high" => "high",
                        _ => "normal",
                    }
                    .to_owned(),
                })
                .collect(),
            todo_total: self.todos.len(),
            net_worth: format_brl(net_worth),
            account_count: self.finances.accounts.len(),
            monthly_bills: format_brl(monthly_bills),
            active_subscriptions: self.finances.bills.iter().filter(|b| b.is_active).count(),
            notes: self
                .notes
                .iter()
                .filter(|n| n.is_pinned && !n.is_trashed && !n.is_archived)
                .take(NOTE_PREVIEW)
                .map(|n| NoteCard {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    preview: if n.content.is_empty() {
                        "Empty note...".to_owned()
                    } else {
                        n.content.clone()
                    },
                })
                .collect(),
            note_total: self.notes.len(),
            quick_task: self.quick_task.clone(),
            loading: self.loading,
            error: self.error.clone(),
        }
    }
}

/// Web `isToday`: the due date's local calendar day is today's.
fn is_today(due: Option<&str>, today: Day) -> bool {
    due.and_then(local_day_of).is_some_and(|day| day == today)
}

/// The account balance in BRL. An amount with no rate is counted as it is (web `?? a.balance`).
fn account_in_brl(account: &Account, rates: Option<&Rates>) -> f64 {
    convert_to_brl(account.balance, &account.currency, rates).unwrap_or(account.balance)
}

/// A bill's cost per month in BRL: yearly divided by 12, weekly times 52 divided by 12, then converted.
fn bill_monthly_in_brl(bill: &Bill, rates: Option<&Rates>) -> f64 {
    let monthly = match bill.billing_cycle.as_str() {
        "yearly" => bill.cost / 12.0,
        "weekly" => bill.cost * 52.0 / 12.0,
        _ => bill.cost,
    };
    convert_to_brl(monthly, &bill.currency, rates).unwrap_or(monthly)
}

/// `Intl.NumberFormat("pt-BR", { style: "currency", currency: "BRL" })`: "R$ 1.234,50", rounded to centavos.
pub fn format_brl(amount: f64) -> String {
    let cents = (amount.abs() * 100.0).round() as i64;
    let sign = if amount < 0.0 && cents != 0 { "-" } else { "" };
    let whole = group_thousands(cents / 100);
    format!("{sign}R$ {whole},{:02}", cents % 100)
}

fn group_thousands(value: i64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

/// The long date the header shows, in English: "Sunday, November 22, 2026".
pub fn long_date(day: Day) -> String {
    const WEEKDAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MONTHS: [&str; 12] = [
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
    format!(
        "{}, {} {}, {}",
        WEEKDAYS[day.weekday as usize % 7],
        MONTHS[(day.month as usize + 11) % 12],
        day.day,
        day.year
    )
}

type Notify = Arc<dyn Fn(OverviewView) + Send + Sync>;

struct Shared {
    api: Arc<dyn OverviewApi>,
    state: Mutex<State>,
    notify: Notify,
}

/// The Overview page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct OverviewController {
    shared: Arc<Shared>,
}

impl OverviewController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn OverviewApi>,
        notify: impl Fn(OverviewView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::default(),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> OverviewView {
        self.render(&self.lock())
    }

    /// Loads the todos, finances, notes and rates in parallel. The loading flag clears when all four have answered.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let remaining = Arc::new(AtomicUsize::new(4));

        let me = self.clone();
        let left = remaining.clone();
        self.shared.api.todos(Box::new(move |result| {
            me.finish_load(&left, |s| match result {
                Ok(todos) => s.todos = todos,
                Err(e) => s.error = Some(e.to_string()),
            });
        }));

        let me = self.clone();
        let left = remaining.clone();
        self.shared.api.finances(Box::new(move |result| {
            me.finish_load(&left, |s| match result {
                Ok(data) => s.finances = data,
                Err(e) => s.error = Some(e.to_string()),
            });
        }));

        let me = self.clone();
        let left = remaining.clone();
        self.shared.api.notes(Box::new(move |result| {
            me.finish_load(&left, |s| match result {
                Ok(notes) => s.notes = notes,
                Err(e) => s.error = Some(e.to_string()),
            });
        }));

        let me = self.clone();
        self.shared.api.rates(Box::new(move |rates| {
            me.finish_load(&remaining, |s| s.rates = rates.ok().flatten());
        }));
    }

    /// Applies one load's answer. The last answer clears the loading flag.
    fn finish_load(&self, remaining: &AtomicUsize, apply: impl FnOnce(&mut State)) {
        let last = remaining.fetch_sub(1, Ordering::SeqCst) == 1;
        self.change(|s| {
            apply(s);
            if last {
                s.loading = false;
            }
        });
    }

    /// Sets the quick-task text field.
    pub fn set_quick_task(&self, title: &str) {
        self.change(|s| s.quick_task = title.to_owned());
    }

    /// Adds today's quick task (trimmed title, "General", medium priority, due today in UTC like the web's
    /// `toISOString`). Returns `false`, and calls nothing, when the title is blank. On success the task goes to the
    /// top of the list and the field clears.
    pub fn add_quick_task(&self) -> bool {
        let title = {
            let state = self.lock();
            state.quick_task.trim().to_owned()
        };
        if title.is_empty() {
            return false;
        }
        let me = self.clone();
        self.shared.api.create_quick(
            &title,
            &utc_today_iso(),
            Box::new(move |result| match result {
                Ok(todo) => me.change(|s| {
                    s.todos.insert(0, todo);
                    s.quick_task.clear();
                }),
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// Ticks or unticks todo `id`. The list updates at once; a refused write reloads the page.
    pub fn toggle_todo(&self, id: &str) -> bool {
        let next = {
            let state = self.lock();
            match state.todos.iter().find(|t| t.id == id) {
                Some(todo) => !todo.is_completed,
                None => return false,
            }
        };
        self.change(|s| {
            for todo in s.todos.iter_mut().filter(|t| t.id == id) {
                todo.is_completed = next;
            }
        });
        let me = self.clone();
        self.shared.api.set_completed(
            id,
            next,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.fail(e);
                    me.refresh();
                }
            }),
        );
        true
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn render(&self, state: &State) -> OverviewView {
        let today = today();
        state.view(today, long_date(today))
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| s.error = Some(error.to_string()));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, update: impl FnOnce(&mut State)) {
        let view = {
            let mut state = self.lock();
            update(&mut state);
            self.render(&state)
        };
        (self.shared.notify)(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};

    /// Answers each read from scripted data and records the writes.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        todos: Mutex<Vec<Todo>>,
        finances: Mutex<FinanceData>,
        notes: Mutex<Vec<Note>>,
        rates: Mutex<Option<Rates>>,
        writes: Mutex<VecDeque<Result<(), ApiError>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn next_write(&self) -> Result<(), ApiError> {
            self.writes.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    impl OverviewApi for Arc<Fake> {
        fn todos(&self, on_done: Done<Vec<Todo>>) {
            self.record("todos".to_owned());
            on_done(Ok(self.todos.lock().unwrap().clone()));
        }

        fn finances(&self, on_done: Done<FinanceData>) {
            self.record("finances".to_owned());
            on_done(Ok(self.finances.lock().unwrap().clone()));
        }

        fn notes(&self, on_done: Done<Vec<Note>>) {
            self.record("notes".to_owned());
            on_done(Ok(self.notes.lock().unwrap().clone()));
        }

        fn rates(&self, on_done: Done<Option<Rates>>) {
            self.record("rates".to_owned());
            on_done(Ok(self.rates.lock().unwrap().clone()));
        }

        fn create_quick(&self, title: &str, due_date: &str, on_done: Done<Todo>) {
            self.record(format!("create:{title}:{due_date}"));
            match self.next_write() {
                Ok(()) => on_done(Ok(todo("new", title, false, "medium"))),
                Err(e) => on_done(Err(e)),
            }
        }

        fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>) {
            self.record(format!("toggle:{id}:{is_completed}"));
            on_done(self.next_write());
        }
    }

    fn todo(id: &str, title: &str, completed: bool, priority: &str) -> Todo {
        Todo {
            id: id.to_owned(),
            title: title.to_owned(),
            category: "Home".to_owned(),
            priority: priority.to_owned(),
            is_completed: completed,
            due_date: None,
        }
    }

    fn note(id: &str, pinned: bool, trashed: bool, content: &str) -> Note {
        Note {
            id: id.to_owned(),
            title: format!("note {id}"),
            content: content.to_owned(),
            tags: Vec::new(),
            color: "neutral".to_owned(),
            is_pinned: pinned,
            is_archived: false,
            is_trashed: trashed,
            updated_at: None,
        }
    }

    fn controller(fake: &Arc<Fake>) -> (OverviewController, Arc<Mutex<Vec<OverviewView>>>) {
        let seen: Arc<Mutex<Vec<OverviewView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn OverviewApi> = Arc::new(fake.clone());
        let c = OverviewController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn loaded(fake: &Arc<Fake>) -> OverviewController {
        let (c, _) = controller(fake);
        c.refresh();
        c
    }

    #[test]
    fn refresh_loads_all_four_sources_and_clears_loading() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(!c.view().loading);
        assert_eq!(fake.calls().len(), 4);
    }

    #[test]
    fn counts_pending_urgent_and_todo_totals() {
        let fake: Arc<Fake> = Arc::default();
        *fake.todos.lock().unwrap() = vec![
            todo("a", "one", false, "urgent"),
            todo("b", "two", false, "high"),
            todo("c", "three", false, "low"),
            todo("d", "done", true, "urgent"),
        ];
        let c = loaded(&fake);
        let view = c.view();
        assert_eq!(view.pending_count, 3);
        assert_eq!(view.urgent_count, 2, "completed urgent tasks do not count");
        assert_eq!(view.todo_total, 4);
        assert_eq!(view.tasks[0].tone, "urgent");
        assert_eq!(view.tasks[2].tone, "normal");
    }

    #[test]
    fn task_list_is_capped_at_six() {
        let fake: Arc<Fake> = Arc::default();
        *fake.todos.lock().unwrap() = (0..8)
            .map(|i| todo(&i.to_string(), "t", false, "medium"))
            .collect();
        let c = loaded(&fake);
        assert_eq!(c.view().tasks.len(), TASK_PREVIEW);
        assert_eq!(c.view().pending_count, 8);
    }

    #[test]
    fn net_worth_converts_accounts_and_falls_back_without_a_rate() {
        let fake: Arc<Fake> = Arc::default();
        fake.finances.lock().unwrap().accounts = vec![
            Account {
                balance: 1000.0,
                currency: "BRL".into(),
                ..Account::default()
            },
            Account {
                balance: 10.0,
                currency: "USD".into(),
                ..Account::default()
            },
        ];
        let mut rates = BTreeMap::new();
        rates.insert("USD".to_owned(), 5.0);
        *fake.rates.lock().unwrap() = Some(Rates { rates });
        let c = loaded(&fake);
        assert_eq!(c.view().net_worth, "R$ 1.002,00");
        assert_eq!(c.view().account_count, 2);

        *fake.rates.lock().unwrap() = None;
        let c = loaded(&fake);
        assert_eq!(
            c.view().net_worth,
            "R$ 1.010,00",
            "no rate: USD counted as-is"
        );
    }

    #[test]
    fn monthly_bills_normalise_each_cycle() {
        let fake: Arc<Fake> = Arc::default();
        fake.finances.lock().unwrap().bills = vec![
            Bill {
                cost: 120.0,
                currency: "BRL".into(),
                billing_cycle: "yearly".into(),
                is_active: true,
                ..Bill::default()
            },
            Bill {
                cost: 10.0,
                currency: "BRL".into(),
                billing_cycle: "weekly".into(),
                is_active: true,
                ..Bill::default()
            },
            Bill {
                cost: 99.0,
                currency: "BRL".into(),
                billing_cycle: "monthly".into(),
                is_active: false,
                ..Bill::default()
            },
        ];
        let c = loaded(&fake);
        let view = c.view();
        assert_eq!(view.monthly_bills, "R$ 53,33", "10 + 10*52/12 = 53.33");
        assert_eq!(view.active_subscriptions, 2);
    }

    #[test]
    fn pinned_notes_skip_trashed_and_show_three() {
        let fake: Arc<Fake> = Arc::default();
        *fake.notes.lock().unwrap() = vec![
            note("1", true, false, "first"),
            note("2", false, false, "not pinned"),
            note("3", true, true, "trashed"),
            note("4", true, false, ""),
            note("5", true, false, "fifth"),
            note("6", true, false, "sixth"),
        ];
        let c = loaded(&fake);
        let view = c.view();
        let previews: Vec<&str> = view.notes.iter().map(|n| n.preview.as_str()).collect();
        assert_eq!(previews, ["first", "Empty note...", "fifth"]);
        assert_eq!(view.note_total, 6);
    }

    #[test]
    fn quick_task_refuses_blank_and_prepends_on_success() {
        let fake: Arc<Fake> = Arc::default();
        *fake.todos.lock().unwrap() = vec![todo("a", "old", false, "medium")];
        let c = loaded(&fake);
        c.set_quick_task("   ");
        assert!(!c.add_quick_task());
        c.set_quick_task("  Call mum  ");
        assert!(c.add_quick_task());
        let calls = fake.calls();
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with("create:Call mum:"))
        );
        let view = c.view();
        assert_eq!(view.todo_total, 2);
        assert_eq!(view.tasks[0].title, "Call mum");
        assert!(view.quick_task.is_empty());
    }

    #[test]
    fn failed_quick_task_keeps_the_text_and_shows_the_error() {
        let fake: Arc<Fake> = Arc::default();
        let c = loaded(&fake);
        fake.writes.lock().unwrap().push_back(Err(ApiError::Status {
            code: 500,
            body: "boom".into(),
        }));
        c.set_quick_task("keep me");
        c.add_quick_task();
        let view = c.view();
        assert_eq!(view.quick_task, "keep me");
        assert_eq!(view.error.as_deref(), Some("HTTP 500: boom"));
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn toggle_flips_the_todo_and_sends_the_new_state() {
        let fake: Arc<Fake> = Arc::default();
        *fake.todos.lock().unwrap() = vec![todo("a", "one", false, "medium")];
        let c = loaded(&fake);
        assert!(c.toggle_todo("a"));
        assert!(fake.calls().contains(&"toggle:a:true".to_owned()));
        assert_eq!(c.view().pending_count, 0);
        assert!(!c.toggle_todo("missing"));
    }

    #[test]
    fn refused_toggle_reloads_the_page() {
        let fake: Arc<Fake> = Arc::default();
        *fake.todos.lock().unwrap() = vec![todo("a", "one", false, "medium")];
        let c = loaded(&fake);
        fake.writes
            .lock()
            .unwrap()
            .push_back(Err(ApiError::Transport("down".into())));
        c.toggle_todo("a");
        let calls = fake.calls();
        let tail: Vec<String> = calls[calls.len() - 4..].to_vec();
        assert_eq!(tail, ["todos", "finances", "notes", "rates"]);
        assert_eq!(c.view().pending_count, 1, "the server still has it open");
    }

    #[test]
    fn today_check_uses_the_local_day() {
        let today = today();
        let due = format!("{:04}-{:02}-{:02}", today.year, today.month, today.day);
        assert!(is_today(Some(&due), today));
        assert!(!is_today(None, today));
        assert!(!is_today(Some("1999-01-01"), today));
    }

    #[test]
    fn format_brl_matches_pt_br() {
        assert_eq!(format_brl(0.0), "R$ 0,00");
        assert_eq!(format_brl(1234.5), "R$ 1.234,50");
        assert_eq!(format_brl(-3.0), "-R$ 3,00");
    }

    #[test]
    fn long_date_reads_in_english() {
        let day = Day {
            year: 2026,
            month: 11,
            day: 22,
            weekday: 0,
        };
        assert_eq!(long_date(day), "Sunday, November 22, 2026");
    }
}
