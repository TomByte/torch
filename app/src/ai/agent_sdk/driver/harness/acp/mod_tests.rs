use serde_json::json;
use url::Url;

use super::attachments::ResolvedFile;
use super::protocol::{ContentBlock, PromptCapabilities};
use super::*;

#[test]
fn initial_prompt_links_files_after_server_composed_text() {
    let dir = std::env::temp_dir().join("acp-initial-attachments");
    let screenshot = dir.join("image #雪%.png");
    let notes = dir.join("notes.txt");
    let harness = AcpHarness::new(Harness::Claude).unwrap();
    let text = harness.initial_prompt_text(
        "inspect these",
        Some("system"),
        Some("recovery"),
        Some("context"),
    );
    let files = [
        ResolvedFile {
            name: "image #雪%.png".to_owned(),
            path: screenshot.clone(),
            mime_type: "image/png".to_owned(),
            contents: None,
        },
        ResolvedFile {
            name: "notes.txt".to_owned(),
            path: notes.clone(),
            mime_type: "text/plain".to_owned(),
            contents: None,
        },
    ];

    let blocks =
        attachments::prompt_content(PromptCapabilities::default(), text, Vec::new(), &files);

    assert_eq!(blocks.len(), 3);
    let ContentBlock::ResourceLink { uri: image_uri, .. } = &blocks[1] else {
        panic!("expected an image resource link");
    };
    let ContentBlock::ResourceLink { uri: notes_uri, .. } = &blocks[2] else {
        panic!("expected a non-image resource link");
    };
    assert_eq!(
        serde_json::to_value(&blocks).unwrap(),
        json!([
            {"type": "text", "text": "recovery\n\ncontext\n\nsystem\n\ninspect these"},
            {"type": "resource_link", "uri": image_uri, "name": "image #雪%.png"},
            {"type": "resource_link", "uri": notes_uri, "name": "notes.txt"}
        ])
    );
    assert_eq!(
        Url::parse(image_uri).unwrap().to_file_path().unwrap(),
        screenshot
    );
    assert_eq!(
        Url::parse(notes_uri).unwrap().to_file_path().unwrap(),
        notes
    );
    assert!(image_uri.contains("image%20%23%E9%9B%AA%25.png"));
}

#[test]
fn initial_prompt_without_files_keeps_the_existing_text() {
    let harness = AcpHarness::new(Harness::Codex).unwrap();

    let text = harness.initial_prompt_text("hello", Some("staged in config"), Some(" "), None);

    assert_eq!(text, "hello");
}
