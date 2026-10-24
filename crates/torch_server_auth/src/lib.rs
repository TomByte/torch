pub mod anonymous_id;
pub mod auth_state;
pub mod credentials;
pub mod user;

pub use auth_state::AuthStateProvider;

/// Prefix for API keys used in authentication.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub const API_KEY_PREFIX: &str = "wk-";
