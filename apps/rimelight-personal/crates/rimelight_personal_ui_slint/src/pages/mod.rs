//! Glue for the feature pages. One module per page, named after the route (`/todos` -> `todos`). Each module builds a
//! feature controller, wires the page's Slint global (ui/pages/<page>.slint) to it, and fetches the page's data on
//! activate. See crates/features/README.md for the pattern.

pub mod finances;
pub mod focus;
pub mod habits;
pub mod health;
pub mod housing;
pub mod inbox;
pub mod music;
pub mod notebooks;
pub mod notes;
pub mod overview;
pub mod projects;
pub mod storage;
pub mod todos;
pub mod watchlist;

#[cfg(test)]
mod tests {
    /// The headless Slint testing backend is installed once per process and lives on the thread that installs it, while
    /// libtest runs each test on its own thread. So every page's headless scenario runs from this one test.
    #[test]
    fn page_scenarios() {
        i_slint_backend_testing::init_integration_test_with_mock_time();
        crate::shell::tests::scenario();
        super::focus::tests::scenario();
        super::todos::tests::scenario();
        super::habits::tests::scenario();
        super::watchlist::tests::scenario();
        super::music::tests::scenario();
        super::notebooks::tests::scenario();
    }
}
