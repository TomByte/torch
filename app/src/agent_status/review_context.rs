use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use torch_editor::render::model::LineCount;

use crate::code_review::comments::{AttachedReviewComment, ReviewCommentBatch};

/// A simplified diff hunk for use in DiffSet attachments
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSetHunk {
    pub line_range: Range<LineCount>,
    pub diff_content: String,
    pub lines_added: u32,
    pub lines_removed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentReviewCommentBatch {
    /// The review comments in this batch. Uses `code_review::comments::ReviewComment`
    /// because it contains full target information needed for API conversion and UI rendering.
    pub comments: Vec<AttachedReviewComment>,
    /// All diff hunks that have comments in this batch attached to them, grouped by file name.
    pub diff_set: HashMap<String, Vec<DiffSetHunk>>,
}

impl AgentReviewCommentBatch {
    pub fn review_comments(&self) -> ReviewCommentBatch {
        ReviewCommentBatch::from_comments(self.comments.clone())
    }
}
