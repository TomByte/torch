use super::derive_http_origin_from_ws_url;

#[test]
fn wss_becomes_https_and_strips_path() {
    let got = derive_http_origin_from_ws_url("wss://rtc.cloud-disabled.torch.invalid/graphql/v2");
    assert_eq!(
        got.as_deref(),
        Some("https://rtc.cloud-disabled.torch.invalid")
    );
}

#[test]
fn ws_becomes_http_and_preserves_port() {
    let got = derive_http_origin_from_ws_url("ws://localhost:8080/graphql/v2");
    assert_eq!(got.as_deref(), Some("http://localhost:8080"));
}

#[test]
fn unparseable_input_returns_none() {
    assert!(derive_http_origin_from_ws_url("not a url").is_none());
    assert!(derive_http_origin_from_ws_url("https://cloud-disabled.torch.invalid").is_none());
}
