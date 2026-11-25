//! The sidebar tree: `NAV` from `crates/app/src/shell.rs`, with each egui glyph replaced by its Lucide name. Projects is
//! omitted, as the user dropped it. `ASSETS` is the footer link egui drew below the sidebar list.

use crate::{Item, Link, Section};

const fn link(label: &'static str, route: &'static str, icon: &'static str) -> Link {
    Link { label, route, icon }
}

/// The footer link above the user row.
pub const ASSETS: Link = link("Assets", "/storage", "folder-open");

/// Every sidebar section, in order.
pub const NAV: &[Section] = &[
    Section {
        title: None,
        items: &[Item {
            link: link("Overview", "/", "layout-dashboard"),
            subs: &[],
        }],
    },
    Section {
        title: Some("Tracking"),
        items: &[
            Item {
                link: link("Inbox", "/inbox", "inbox"),
                subs: &[],
            },
            Item {
                link: link("Annotations", "/notes", "notebook-pen"),
                subs: &[
                    link("Notes", "/notes", "sticky-note"),
                    link("Notebooks", "/notebooks", "notebook"),
                ],
            },
            Item {
                link: link("Todos", "/todos", "square-check-big"),
                subs: &[],
            },
            Item {
                link: link("Habits", "/habits", "circle-check"),
                subs: &[],
            },
        ],
    },
    Section {
        title: Some("Management"),
        items: &[
            Item {
                link: link("Finances", "/finances", "wallet"),
                subs: &[
                    link("Overview", "/finances", "wallet"),
                    link("Transactions", "/finances/transactions", "receipt"),
                    link("Accounts", "/finances/accounts", "landmark"),
                    link("Incomes", "/finances/incomes", "arrow-down-left"),
                    link("Bills", "/finances/bills", "credit-card"),
                    link("Budgets", "/finances/budgets", "chart-pie"),
                    link("Debts & Loans", "/finances/debts", "scale"),
                    link("Wishlist", "/finances/wishlist", "gift"),
                ],
            },
            Item {
                link: link("Projects", "/projects", "folder-kanban"),
                subs: &[],
            },
            Item {
                link: link("Housing", "/housing", "house"),
                subs: &[
                    link("Overview", "/housing", "house"),
                    link("Pets", "/housing/pets", "heart"),
                    link("Groceries", "/housing/groceries", "shopping-cart"),
                    link("Wishlist", "/housing/wishlist", "sparkles"),
                ],
            },
        ],
    },
    Section {
        title: Some("Leisure"),
        items: &[
            Item {
                link: link("Health & Fitness", "/health", "activity"),
                subs: &[
                    link("Overview", "/health", "activity"),
                    link("Fasting", "/health/fasting", "timer"),
                    link("Workouts & Mobility", "/health/workouts", "dumbbell"),
                    link("Measurements", "/health/measurements", "scale"),
                ],
            },
            Item {
                link: link("Music", "/music", "music"),
                subs: &[
                    link("Overview", "/music", "music"),
                    link("Piano", "/music/piano", "music-2"),
                    link("Guitar", "/music/guitar", "disc"),
                    link("DJ Sets", "/music/dj", "radio"),
                ],
            },
            Item {
                link: link("Watchlist", "/watchlist", "film"),
                subs: &[
                    link("All Media", "/watchlist", "film"),
                    link("Movies", "/watchlist/movies", "film"),
                    link("TV Series", "/watchlist/series", "tv"),
                    link("Books", "/watchlist/books", "book-open"),
                ],
            },
        ],
    },
];
