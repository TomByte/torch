use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ImageContext {
    /// Base64-encoded image data.
    pub data: String,

    /// MIME type of the media content (e.g., "image/jpeg", "image/png")
    pub mime_type: String,

    pub file_name: String,

    /// Whether this image was exported from Figma, detected via
    /// the `Software: Figma` PNG metadata field.
    #[serde(default)]
    pub is_figma: bool,
}

impl std::fmt::Debug for ImageContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // We log dispatching typed actions (with `ImageContext` as an argument) and we don't want
        // to log any UGC in prod.
        f.debug_struct("ImageContext")
            .field("data", &"REDACTED_B64_IMAGE_DATA_UGC")
            .field("mime_type", &self.mime_type)
            .field("file_name", &"REDACTED_FILE_NAME_UGC")
            .finish()
    }
}
