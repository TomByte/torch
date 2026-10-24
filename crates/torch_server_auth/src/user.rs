use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirebaseAuthTokens {
    /// ID tokens are Firebase tokens, which are short-lived tokens that are used to authenticate
    /// requests to the server. These are obtained by exchanging long-lived refresh tokens.
    pub id_token: String,
    /// Refresh tokens are long-lived tokens that can be exchanged for short-lived access tokens
    /// (stored in the id_token field). We use the refresh token to get a new ID token when the
    /// current one expires.
    /// Note that there are two types of refresh tokens we store in this field:
    /// "Refresh tokens": these are used for logged-in users.
    /// "Custom tokens": these are used for anonymous firebase users.
    pub refresh_token: String,
    /// When the ID token expires. If the token has expired, or will expire soon, we should
    /// fetch a new ID token using the user's refresh token.
    pub expiration_time: DateTime<FixedOffset>,
}

impl FirebaseAuthTokens {
    pub fn from_response(
        id_token: String,
        refresh_token: String,
        expires_in: String,
    ) -> Result<Self, anyhow::Error> {
        let local_time = Local::now();
        Ok(Self {
            id_token,
            expiration_time: local_time.with_timezone(local_time.offset())
                + chrono::Duration::seconds(
                    expires_in.parse::<i64>().map_err(anyhow::Error::from)?,
                ),
            refresh_token,
        })
    }
}
