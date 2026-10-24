use std::collections::HashMap;

use torch_editor::render::model::LineCount;

use crate::agent_status::DiffSetHunk;
use crate::code_review::diff_state::{DiffLineType, FileDiff};

/// Converts file diffs into a map keyed by repo-relative path strings.
pub fn convert_file_diffs_to_diffset_hunks<'a, I>(files: I) -> HashMap<String, Vec<DiffSetHunk>>
where
    I: Iterator<Item = &'a FileDiff>,
{
    let mut file_diffs: HashMap<String, Vec<DiffSetHunk>> = HashMap::new();

    for file_diff in files {
        let repo_relative_path = file_diff.file_path.clone();

        let mut file_hunks = Vec::new();
        for hunk in file_diff.hunks.iter() {
            // Format the diff content for this hunk
            let mut diff_lines = Vec::new();
            let mut lines_added = 0;
            let mut lines_removed = 0;
            for line in &hunk.lines {
                let prefix = match line.line_type {
                    DiffLineType::Add => {
                        lines_added += 1;
                        "+"
                    }
                    DiffLineType::Delete => {
                        lines_removed += 1;
                        "-"
                    }
                    DiffLineType::Context => "",
                    DiffLineType::HunkHeader => continue,
                };
                diff_lines.push(format!("{}{}", prefix, line.text));
            }
            let diff_content = diff_lines.join("\n");

            // Create line range using LineCount: Note that git lines are 1-based and LineCount is 0-based
            let line_range = LineCount::from(hunk.new_start_line.saturating_sub(1))
                ..LineCount::from(hunk.new_start_line.saturating_sub(1) + hunk.new_line_count);

            file_hunks.push(DiffSetHunk {
                line_range,
                diff_content,
                lines_added,
                lines_removed,
            });
        }

        if !file_hunks.is_empty() {
            file_diffs.insert(repo_relative_path, file_hunks);
        }
    }

    file_diffs
}
