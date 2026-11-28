//! Shared models and HTTP client for the personal API, the Cloudflare Access sign-in, and FX rates.

pub mod api;
pub mod auth;
/// The Cloudflare OAuth sign-in for the desktop setup. Native only: the browser, a loopback listener and native crates.
#[cfg(not(target_arch = "wasm32"))]
pub mod cf_oauth;
/// The live Cloudflare API client for the setup. Native only.
#[cfg(not(target_arch = "wasm32"))]
pub mod cloudflare_api;
pub mod finance;
pub mod habits;
pub mod health;
pub mod housing;
pub mod inbox;
pub mod instance;
pub mod local_date;
pub mod music;
pub mod notebooks;
pub mod notes;
pub mod projects;
pub mod provision;
pub mod rates;
/// The desktop Access CLI login. Native only: the browser and a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub mod settings;
/// The desktop Access CLI login. Native only: the browser, the OS and the native crates.
#[cfg(not(target_arch = "wasm32"))]
pub mod signin;
pub mod storage;
pub mod todo;
pub mod watchlist;
/// The browser's Access session: navigation and the cookie probe. Wasm only.
#[cfg(target_arch = "wasm32")]
pub mod web_session;

pub use api::{ApiClient, ApiError};
pub use api::{BearerSource, HeaderToken};
pub use auth::{ACCESS_HEADER, Auth, AuthError, MemoryStore, SessionStore};
pub use finance::{Account, Bill, FinanceData};
pub use habits::{HabitAction, HabitData};
pub use instance::InstanceConfig;
pub use notes::{NewNote, Note, NoteBatchAction, NotePatch};
pub use rates::{BASE_CURRENCY, Rates, convert_to_brl, fetch_rates};
pub use todo::NewDatedTodo;
pub use todo::{Ack, Todo};
