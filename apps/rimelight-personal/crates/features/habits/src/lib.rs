//! The Habits page controller (ported from `crates/app/src/habits.rs` and the web `HabitsTracker`).
//!
//! Plain Rust with no Slint. The controller holds the page state (the tracker, the selected year and month, the
//! calendar or streaks view, the category-delete confirmation, the loading flag, the error message), calls the habit
//! endpoints through [`HabitApi`], and reports each state change as a [`HabitsView`]. The Slint glue in
//! `apps/desktop/src/pages/habits.rs` turns those views into the models of `ui/habits.slint`.
//!
//! Threading: as in the Todos controller. The API calls return at once and answer later on another thread. The
//! controller locks its state only around reads and writes, never across an API call, and calls `notify` after the
//! lock is released.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use rimelight_personal_core::habits::{HabitAction, HabitData};
use rimelight_personal_core::{ApiClient, ApiError};

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@rimelight_personal_habits`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Month names, January first, as the web's month tabs and the egui page list them.
pub const MONTHS: [&str; 12] = [
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

/// The API boundary: the tracker read and the mutations. Every mutation answers with the whole tracker, which
/// replaces the local copy. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait HabitApi: Send + Sync {
    fn habits(&self, on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>);
    fn act(
        &self,
        action: &HabitAction<'_>,
        on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>,
    );
}

impl HabitApi for ApiClient {
    fn habits(&self, on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>) {
        ApiClient::habits(self, on_done);
    }

    fn act(
        &self,
        action: &HabitAction<'_>,
        on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>,
    ) {
        ApiClient::habit_action(self, action, on_done);
    }
}

/// The live API for `api_base`, for example `https://example.com/api/personal`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn HabitApi> {
    Arc::new(ApiClient::new(api_base))
}

/// Current `(year, month, day)` in UTC, the month 1-based. The desktop shell passes it to [`HabitsController::new`].
pub fn today() -> (i32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_from_days((secs / 86_400) as i64)
}

/// One day cell: the calendar grid, or the 52-week grid of the streaks view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayView {
    /// `YYYY-MM-DD`, the key the server stores.
    pub date: String,
    /// The day of the month, 1-based.
    pub day: u32,
    pub done: bool,
}

/// One habit: its name, its calendar cells for the selected month, and its streak and completion figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HabitView {
    pub id: String,
    pub name: String,
    pub days: Vec<DayView>,
    /// Completed days in the selected month, as a percentage of the month's days (rounded, as the web shows it).
    pub month_percent: u32,
    /// Consecutive completed days ending today, or yesterday when today is not done yet.
    pub current_streak: u32,
    /// The longest run of consecutive completed days, never less than the current streak.
    pub best_streak: u32,
    /// Completed days in the selected year.
    pub year_completed: u32,
    /// `year_completed` as a percentage of 365 (rounded), as the web's streaks view shows it.
    pub year_percent: u32,
    /// A cell for each day of the selected year, for the 52-week grid of the streaks view.
    pub year_cells: Vec<DayView>,
}

/// One category of the selected year and its habits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    pub id: String,
    pub name: String,
    pub habits: Vec<HabitView>,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HabitsView {
    /// The year tabs, in the order the server returns them.
    pub years: Vec<i32>,
    pub selected_year: Option<i32>,
    /// False when no year is selected (none exists yet): the page then shows the "add a year" hint and no categories.
    pub has_year: bool,
    /// 1-based.
    pub selected_month: u32,
    /// True in the streaks view, false in the calendar view.
    pub streaks_mode: bool,
    pub categories: Vec<CategoryView>,
    pub loading: bool,
    /// The message of the last failed call, until the user dismisses it.
    pub error: Option<String>,
    /// Set while the category-delete confirmation is open: the name of the category to delete.
    pub pending_delete_category: Option<String>,
}

struct State {
    data: HabitData,
    loading: bool,
    error: Option<String>,
    selected_year: Option<i32>,
    selected_month: u32,
    streaks_mode: bool,
    /// `(category id, category name)` awaiting the user's confirmation.
    pending_delete: Option<(String, String)>,
}

impl State {
    fn view(&self, today: (i32, u32, u32)) -> HabitsView {
        let year = self
            .selected_year
            .and_then(|y| self.data.years.iter().find(|h| h.year == y));
        let categories = year.map_or_else(Vec::new, |year| {
            let days = days_in_month(year.year, self.selected_month);
            let year_days = days_in_year(year.year);
            year.categories
                .iter()
                .map(|cat| CategoryView {
                    id: cat.id.clone(),
                    name: cat.name.clone(),
                    habits: cat
                        .habits
                        .iter()
                        .map(|habit| {
                            let cell = |date: String, day: u32| DayView {
                                done: habit.completed_dates.contains(&date),
                                date,
                                day,
                            };
                            let month_cells: Vec<DayView> = (1..=days)
                                .map(|day| {
                                    cell(date_string(year.year, self.selected_month, day), day)
                                })
                                .collect();
                            let month_done = month_cells.iter().filter(|c| c.done).count() as u32;
                            let streak = streak(&habit.completed_dates, today);
                            let year_done = habit
                                .completed_dates
                                .iter()
                                .filter(|d| parse_date(d).is_some_and(|(y, _, _)| y == year.year))
                                .count() as u32;
                            HabitView {
                                id: habit.id.clone(),
                                name: habit.name.clone(),
                                days: month_cells,
                                month_percent: percent(month_done, days),
                                current_streak: streak.current,
                                best_streak: streak.best,
                                year_completed: year_done,
                                year_percent: percent(year_done, 365),
                                year_cells: (1..=year_days)
                                    .map(|n| {
                                        let (m, d) = day_of_year(year.year, n);
                                        cell(date_string(year.year, m, d), d)
                                    })
                                    .collect(),
                            }
                        })
                        .collect(),
                })
                .collect()
        });
        HabitsView {
            years: self.data.years.iter().map(|y| y.year).collect(),
            selected_year: self.selected_year,
            has_year: year.is_some(),
            selected_month: self.selected_month,
            streaks_mode: self.streaks_mode,
            categories,
            loading: self.loading,
            error: self.error.clone(),
            pending_delete_category: self.pending_delete.as_ref().map(|(_, name)| name.clone()),
        }
    }
}

type Notify = Arc<dyn Fn(HabitsView) + Send + Sync>;

struct Shared {
    api: Arc<dyn HabitApi>,
    today: (i32, u32, u32),
    state: Mutex<State>,
    notify: Notify,
}

/// The Habits page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct HabitsController {
    shared: Arc<Shared>,
}

impl HabitsController {
    /// `today` is `(year, month, day)`, see [`today`]. It picks the starting month, the day the streaks count back
    /// from, and, once the tracker arrives, the starting year. `notify` receives the new view after every state change,
    /// possibly on a worker thread.
    pub fn new(
        api: Arc<dyn HabitApi>,
        today: (i32, u32, u32),
        notify: impl Fn(HabitsView) + Send + Sync + 'static,
    ) -> Self {
        let state = State {
            data: HabitData::default(),
            loading: false,
            error: None,
            selected_year: None,
            selected_month: today.1,
            streaks_mode: false,
            pending_delete: None,
        };
        Self {
            shared: Arc::new(Shared {
                api,
                today,
                state: Mutex::new(state),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> HabitsView {
        self.lock().view(self.shared.today)
    }

    /// Fetches the tracker. Shows the loading flag until the answer arrives.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared
            .api
            .habits(Box::new(move |result| me.receive(result)));
    }

    /// Selects a year tab. Nothing is sent.
    pub fn select_year(&self, year: i32) {
        self.change(|s| s.selected_year = Some(year));
    }

    /// Selects a month, 1-based. Months outside 1..=12 are ignored.
    pub fn select_month(&self, month: u32) {
        if (1..=12).contains(&month) {
            self.change(|s| s.selected_month = month);
        }
    }

    /// Switches between the calendar view (`false`) and the streaks view (`true`). Nothing is sent.
    pub fn set_streaks_mode(&self, streaks: bool) {
        self.change(|s| s.streaks_mode = streaks);
    }

    /// Adds a year from `text` (trimmed) and selects it at once. Returns `false`, and calls nothing, when the text is
    /// not a number.
    pub fn add_year(&self, text: &str) -> bool {
        let Ok(year) = text.trim().parse::<i32>() else {
            return false;
        };
        self.change(|s| s.selected_year = Some(year));
        self.act(HabitAction::AddYear { year });
        true
    }

    /// Adds a category to the selected year. Returns `false`, and calls nothing, when the name is blank or no year is
    /// selected.
    pub fn add_category(&self, name: &str) -> bool {
        let name = name.trim();
        match self.selected_year() {
            Some(year) if !name.is_empty() => {
                self.act(HabitAction::AddCategory { year, name });
                true
            }
            _ => false,
        }
    }

    /// Asks to delete a category of the selected year. Opens the confirmation (the view's `pending_delete_category`
    /// carries the name); nothing is sent until [`confirm_delete_category`](Self::confirm_delete_category).
    pub fn request_delete_category(&self, cat_id: &str) {
        let Some(year) = self.selected_year() else {
            return;
        };
        let name = self
            .lock()
            .data
            .years
            .iter()
            .find(|y| y.year == year)
            .and_then(|y| y.categories.iter().find(|c| c.id == cat_id))
            .map(|c| c.name.clone());
        if let Some(name) = name {
            self.change(|s| s.pending_delete = Some((cat_id.to_owned(), name)));
        }
    }

    /// Deletes the category the confirmation opened for, and closes it. Calls nothing when none is open.
    pub fn confirm_delete_category(&self) {
        let pending = self.lock().pending_delete.clone();
        self.change(|s| s.pending_delete = None);
        if let (Some((cat_id, _)), Some(year)) = (pending, self.selected_year()) {
            self.act(HabitAction::DeleteCategory {
                year,
                cat_id: &cat_id,
            });
        }
    }

    /// Closes the category-delete confirmation without deleting.
    pub fn cancel_delete_category(&self) {
        self.change(|s| s.pending_delete = None);
    }

    /// Adds a habit to a category of the selected year. Returns `false`, and calls nothing, when the name is blank or
    /// no year is selected.
    pub fn add_habit(&self, cat_id: &str, name: &str) -> bool {
        let name = name.trim();
        match self.selected_year() {
            Some(year) if !name.is_empty() => {
                self.act(HabitAction::AddHabit { year, cat_id, name });
                true
            }
            _ => false,
        }
    }

    /// Deletes a habit of the selected year.
    pub fn delete_habit(&self, cat_id: &str, habit_id: &str) {
        if let Some(year) = self.selected_year() {
            self.act(HabitAction::DeleteHabit {
                year,
                cat_id,
                habit_id,
            });
        }
    }

    /// Flips one day (`YYYY-MM-DD`) of a habit. The local copy flips first; the server's answer then replaces it.
    pub fn toggle_day(&self, cat_id: &str, habit_id: &str, date: &str) {
        let Some(year) = self.selected_year() else {
            return;
        };
        self.change(|s| {
            let habit = s
                .data
                .years
                .iter_mut()
                .filter(|y| y.year == year)
                .flat_map(|y| y.categories.iter_mut())
                .filter(|c| c.id == cat_id)
                .flat_map(|c| c.habits.iter_mut())
                .find(|h| h.id == habit_id);
            if let Some(habit) = habit {
                match habit.completed_dates.iter().position(|d| d == date) {
                    Some(i) => {
                        habit.completed_dates.remove(i);
                    }
                    None => habit.completed_dates.push(date.to_owned()),
                }
            }
        });
        self.act(HabitAction::ToggleDate {
            year,
            cat_id,
            habit_id,
            date,
        });
    }

    /// Clears the error message.
    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn selected_year(&self) -> Option<i32> {
        self.lock().selected_year
    }

    fn act(&self, action: HabitAction<'_>) {
        let me = self.clone();
        self.shared
            .api
            .act(&action, Box::new(move |result| me.receive(result)));
    }

    /// Every answer carries the whole tracker. The first one picks the starting year.
    fn receive(&self, result: Result<HabitData, ApiError>) {
        let today_year = self.shared.today.0;
        self.change(|s| {
            s.loading = false;
            match result {
                Ok(data) => {
                    s.data = data;
                    s.error = None;
                    if s.selected_year.is_none() {
                        s.selected_year = pick_year(&s.data, today_year);
                    }
                }
                Err(e) => s.error = Some(e.to_string()),
            }
        });
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in one callback must not make every later call panic too.
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
            state.view(self.shared.today)
        };
        (self.shared.notify)(view);
    }
}

/// Today's year when the tracker has it, otherwise the first year. `None` when there are no years.
fn pick_year(data: &HabitData, today_year: i32) -> Option<i32> {
    if data.years.iter().any(|y| y.year == today_year) {
        Some(today_year)
    } else {
        data.years.first().map(|y| y.year)
    }
}

/// Current and best streak of one habit, as the web's `calculateStreak` computes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Streak {
    current: u32,
    best: u32,
}

/// The current streak counts back from `today`, or from yesterday when today is not done. The best streak is the
/// longest run of consecutive days and is never shorter than the current one.
fn streak(completed: &[String], today: (i32, u32, u32)) -> Streak {
    let set: HashSet<i64> = completed
        .iter()
        .filter_map(|d| parse_date(d).map(days_from_civil))
        .collect();

    let today_ord = days_from_civil(today);
    let mut check = if set.contains(&today_ord) {
        today_ord
    } else {
        today_ord - 1
    };
    let mut current = 0;
    while set.contains(&check) {
        current += 1;
        check -= 1;
    }

    let mut days: Vec<i64> = set.into_iter().collect();
    days.sort_unstable();
    let mut best = 0;
    let mut run = 0;
    let mut prev: Option<i64> = None;
    for &day in &days {
        run = if prev == Some(day - 1) { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(day);
    }
    Streak {
        current,
        best: best.max(current),
    }
}

/// `part` as a percentage of `whole`, rounded half up (JavaScript's `Math.round` for non-negative numbers).
fn percent(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        0
    } else {
        (part * 200 + whole) / (2 * whole)
    }
}

fn date_string(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// `(year, month, day)` of a `YYYY-MM-DD` string, or `None` when it is not a real date.
fn parse_date(text: &str) -> Option<(i32, u32, u32)> {
    let mut parts = text.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some((year, month, day))
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_year(year: i32) -> u32 {
    if is_leap(year) { 366 } else { 365 }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        _ => 28,
    }
}

/// The `(month, day)` of the `n`th day of `year` (1-based).
fn day_of_year(year: i32, n: u32) -> (u32, u32) {
    let mut left = n;
    for month in 1..=12 {
        let len = days_in_month(year, month);
        if left <= len {
            return (month, left);
        }
        left -= len;
    }
    (12, 31)
}

/// Howard Hinnant's `days_from_civil`: `(year, month, day)` to days since 1970-01-01.
fn days_from_civil((year, month, day): (i32, u32, u32)) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to `(year, month, day)`.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimelight_personal_core::habits::{Habit, HabitCategory, HabitYear};

    /// Records each call and answers at once: the tracker `data`, or the next scripted failure.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        data: Mutex<HabitData>,
        next_error: Mutex<Option<ApiError>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn set_data(&self, data: HabitData) {
            *self.data.lock().unwrap() = data;
        }

        fn reply(&self) -> Result<HabitData, ApiError> {
            let failure = self.next_error.lock().unwrap().take();
            match failure {
                Some(e) => Err(e),
                None => Ok(self.data.lock().unwrap().clone()),
            }
        }
    }

    impl HabitApi for Fake {
        fn habits(&self, on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>) {
            self.calls.lock().unwrap().push("load".to_owned());
            on_done(self.reply());
        }

        fn act(
            &self,
            action: &HabitAction<'_>,
            on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>,
        ) {
            self.calls.lock().unwrap().push(describe(action));
            on_done(self.reply());
        }
    }

    fn describe(action: &HabitAction<'_>) -> String {
        match action {
            HabitAction::AddYear { year } => format!("add-year:{year}"),
            HabitAction::AddCategory { year, name } => format!("add-category:{year}:{name}"),
            HabitAction::DeleteCategory { year, cat_id } => {
                format!("delete-category:{year}:{cat_id}")
            }
            HabitAction::AddHabit { year, cat_id, name } => {
                format!("add-habit:{year}:{cat_id}:{name}")
            }
            HabitAction::DeleteHabit {
                year,
                cat_id,
                habit_id,
            } => format!("delete-habit:{year}:{cat_id}:{habit_id}"),
            HabitAction::ToggleDate {
                year,
                cat_id,
                habit_id,
                date,
            } => format!("toggle-date:{year}:{cat_id}:{habit_id}:{date}"),
        }
    }

    fn habit(id: &str, name: &str, dates: &[&str]) -> Habit {
        Habit {
            id: id.to_owned(),
            name: name.to_owned(),
            completed_dates: dates.iter().map(|d| (*d).to_owned()).collect(),
        }
    }

    /// 2025 is empty; 2026 has one category with one habit, done on 2 November.
    fn sample() -> HabitData {
        HabitData {
            years: vec![
                HabitYear {
                    year: 2025,
                    categories: vec![],
                },
                HabitYear {
                    year: 2026,
                    categories: vec![HabitCategory {
                        id: "c1".to_owned(),
                        name: "Health".to_owned(),
                        habits: vec![habit("h1", "Run", &["2026-11-02"])],
                    }],
                },
            ],
        }
    }

    fn controller(
        fake: &Arc<Fake>,
        today: (i32, u32, u32),
    ) -> (HabitsController, Arc<Mutex<Vec<HabitsView>>>) {
        let seen: Arc<Mutex<Vec<HabitsView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn HabitApi> = fake.clone();
        let c = HabitsController::new(api, today, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    #[test]
    fn refresh_shows_loading_then_picks_todays_year_and_month() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, seen) = controller(&fake, (2026, 11, 22));
        c.refresh();

        let views = seen.lock().unwrap().clone();
        assert!(
            views[0].loading,
            "the first notification is the loading flag"
        );
        let view = c.view();
        assert!(!view.loading);
        assert_eq!(view.years, [2025, 2026]);
        assert_eq!(view.selected_year, Some(2026));
        assert!(view.has_year);
        assert_eq!(view.selected_month, 11);
        assert_eq!(view.categories[0].habits[0].days.len(), 30);
        assert!(
            view.categories[0].habits[0].days[1].done,
            "2 November is done"
        );
        assert_eq!(fake.calls(), ["load"]);
    }

    #[test]
    fn starts_on_the_first_year_when_today_has_none() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2030, 1, 1));
        c.refresh();
        assert_eq!(c.view().selected_year, Some(2025));
        assert!(c.view().has_year);
    }

    #[test]
    fn empty_tracker_shows_no_year() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        let view = c.view();
        assert_eq!(view.selected_year, None);
        assert!(!view.has_year);
        assert!(view.categories.is_empty());
    }

    #[test]
    fn add_year_parses_selects_and_sends() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        assert!(!c.add_year("twenty"));
        assert!(
            fake.calls().is_empty(),
            "a non-number is refused without a call"
        );

        assert!(c.add_year(" 2030 "));
        assert_eq!(fake.calls(), ["add-year:2030"]);
        assert_eq!(c.view().selected_year, Some(2030));
    }

    #[test]
    fn select_month_changes_the_day_count_and_ignores_bad_months() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();

        c.select_month(2);
        assert_eq!(c.view().categories[0].habits[0].days.len(), 28);
        c.select_month(13);
        assert_eq!(c.view().selected_month, 2);
    }

    #[test]
    fn select_year_switches_the_categories_without_a_call() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        assert_eq!(c.view().categories.len(), 1);

        c.select_year(2025);
        let view = c.view();
        assert_eq!(view.selected_year, Some(2025));
        assert!(view.categories.is_empty(), "2025 has no categories");
        assert_eq!(fake.calls(), ["load"], "selecting a year sends nothing");
    }

    #[test]
    fn streaks_mode_toggles_without_a_call() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        assert!(!c.view().streaks_mode);
        c.set_streaks_mode(true);
        assert!(c.view().streaks_mode);
        c.set_streaks_mode(false);
        assert!(!c.view().streaks_mode);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn toggle_day_flips_locally_then_takes_the_server_answer() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();

        // The server answers with the tracker that has 3 November done too.
        let mut answer = sample();
        answer.years[1].categories[0].habits[0]
            .completed_dates
            .push("2026-11-03".to_owned());
        fake.set_data(answer);

        c.toggle_day("c1", "h1", "2026-11-03");
        assert_eq!(fake.calls(), ["load", "toggle-date:2026:c1:h1:2026-11-03"]);
        let days = &c.view().categories[0].habits[0].days;
        assert!(days[2].done, "the answer replaces the local flip");
    }

    #[test]
    fn blank_names_are_refused_without_a_call() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        let before = fake.calls().len();
        assert!(!c.add_category("   "));
        assert!(!c.add_habit("c1", "  "));
        assert_eq!(fake.calls().len(), before);
    }

    #[test]
    fn add_category_and_habit_trim_and_use_the_selected_year() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        assert!(c.add_category("  Money "));
        assert!(c.add_habit("c1", " Read "));
        assert_eq!(
            fake.calls(),
            ["load", "add-category:2026:Money", "add-habit:2026:c1:Read"]
        );
    }

    #[test]
    fn writes_are_refused_when_no_year_is_selected() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        assert!(!c.add_category("Health"));
        assert!(!c.add_habit("c1", "Run"));
        c.request_delete_category("c1");
        c.confirm_delete_category();
        c.delete_habit("c1", "h1");
        c.toggle_day("c1", "h1", "2026-11-02");
        assert!(fake.calls().is_empty());
        assert_eq!(c.view().pending_delete_category, None);
    }

    #[test]
    fn category_delete_waits_for_confirmation() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();

        c.request_delete_category("c1");
        assert_eq!(c.view().pending_delete_category.as_deref(), Some("Health"));
        assert_eq!(
            fake.calls(),
            ["load"],
            "opening the confirmation sends nothing"
        );

        c.cancel_delete_category();
        assert_eq!(c.view().pending_delete_category, None);
        c.confirm_delete_category();
        assert!(
            fake.calls().iter().all(|call| call == "load"),
            "a cancelled delete sends nothing"
        );

        c.request_delete_category("c1");
        c.confirm_delete_category();
        assert_eq!(c.view().pending_delete_category, None);
        assert_eq!(fake.calls(), ["load", "delete-category:2026:c1"]);
    }

    #[test]
    fn delete_habit_sends_the_ids_and_refreshes_the_copy() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        c.delete_habit("c1", "h1");
        assert_eq!(fake.calls(), ["load", "delete-habit:2026:c1:h1"]);
    }

    #[test]
    fn failed_write_sets_error_and_dismiss_clears_it() {
        let fake: Arc<Fake> = Arc::default();
        fake.set_data(sample());
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        *fake.next_error.lock().unwrap() = Some(ApiError::Status {
            code: 500,
            body: "boom".to_owned(),
        });
        c.delete_habit("c1", "h1");
        assert_eq!(c.view().error.as_deref(), Some("HTTP 500: boom"));
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn month_percent_and_year_figures_match_the_web_rounding() {
        let fake: Arc<Fake> = Arc::default();
        // Two completions in November (30 days): 2/30 = 6.67% -> 7. Year: 2 of 365 -> 0.55% -> 1.
        let mut data = sample();
        data.years[1].categories[0].habits[0]
            .completed_dates
            .push("2026-11-03".to_owned());
        fake.set_data(data);
        let (c, _seen) = controller(&fake, (2026, 11, 22));
        c.refresh();
        let habit = &c.view().categories[0].habits[0];
        assert_eq!(habit.month_percent, 7);
        assert_eq!(habit.year_completed, 2);
        assert_eq!(habit.year_percent, 1);
        assert_eq!(habit.year_cells.len(), 365);
        assert_eq!(habit.year_cells[305].date, "2026-11-02");
        assert!(habit.year_cells[305].done);
    }

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let dates =
            |list: &[&str]| -> Vec<String> { list.iter().map(|d| (*d).to_owned()).collect() };
        let today = (2026, 11, 22);

        // Today done: 22, 21, 20 -> current 3.
        assert_eq!(
            streak(&dates(&["2026-11-22", "2026-11-21", "2026-11-20"]), today),
            Streak {
                current: 3,
                best: 3
            }
        );
        // Today not done yet, yesterday done: the streak still counts.
        assert_eq!(
            streak(&dates(&["2026-11-21", "2026-11-20"]), today),
            Streak {
                current: 2,
                best: 2
            }
        );
        // Neither today nor yesterday: current is 0, best is the old run.
        assert_eq!(
            streak(
                &dates(&["2026-11-10", "2026-11-09", "2026-11-08", "2026-11-01"]),
                today
            ),
            Streak {
                current: 0,
                best: 3
            }
        );
        // Best run across a month boundary, and the current run is the best when it is longest.
        assert_eq!(
            streak(
                &dates(&["2026-02-27", "2026-02-28", "2026-03-01", "2026-03-02"]),
                (2026, 3, 2)
            ),
            Streak {
                current: 4,
                best: 4
            }
        );
        // Nothing: zero.
        assert_eq!(
            streak(&[], today),
            Streak {
                current: 0,
                best: 0
            }
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-02-29 is 11016 days after the epoch.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 2026-09-13 is 20_709 days after the epoch.
        assert_eq!(civil_from_days(20_709), (2026, 9, 13));
        assert_eq!(days_from_civil((2026, 9, 13)), 20_709);
        assert_eq!(days_from_civil((2000, 2, 29)), 11_016);
    }

    #[test]
    fn month_lengths_and_padding() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_year(2024), 366);
        assert_eq!(days_in_year(2026), 365);
        assert_eq!(day_of_year(2026, 32), (2, 1));
        assert_eq!(date_string(2026, 3, 7), "2026-03-07");
    }

    #[test]
    fn parse_date_rejects_non_dates() {
        assert_eq!(parse_date("2026-02-29"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("2026-11-2x"), None);
        assert_eq!(parse_date("2026-11-02"), Some((2026, 11, 2)));
    }
}
