#[cfg(any(test, feature = "test-util"))]
pub use torch_server_client::auth::MockAuthClient;
pub use torch_server_client::auth::{
    AuthClient, FetchUserResult, SyncedUserSettings, UserAuthenticationError,
};
