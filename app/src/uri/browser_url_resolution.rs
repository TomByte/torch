#[cfg(target_family = "wasm")]
use url::Url;

#[cfg(target_family = "wasm")]
const BASE_APP_PATH: &str = "/app";

/// Resolves the URL that belongs on the browser address bar. Pure, so the
/// decision is testable apart from the DOM-writing wrapper in
/// `browser_url_handler`.
#[cfg(target_family = "wasm")]
pub(crate) fn resolve_browser_url(
    current_url: Option<Url>,
    requested_url: Option<Url>,
) -> Option<Url> {
    requested_url.or_else(|| base_app_url(current_url))
}

#[cfg(target_family = "wasm")]
fn base_app_url(current_url: Option<Url>) -> Option<Url> {
    let mut new_url = current_url?;
    new_url.set_path(BASE_APP_PATH);
    new_url.set_query(None);
    Some(new_url)
}
