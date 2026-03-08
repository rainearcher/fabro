use crate::sandbox::{format_lines_numbered, Sandbox};
use crate::tool_registry::RegisteredTool;
use crate::truncation::{truncate_output, TruncationMode};
use arc_llm::types::ToolDefinition;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Remove(String),
    Add(String),
    Context(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub context_line: String,
    pub changes: Vec<Change>,
    pub end_of_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOperation {
    Add { path: String, content: String },
    Delete { path: String },
    Update {
        path: String,
        new_path: Option<String>,
        hunks: Vec<Hunk>,
    },
}

fn is_hunk_start(line: &str) -> bool {
    line == "@@" || line.starts_with("@@ ")
}

fn extract_context_line(line: &str) -> String {
    if line == "@@" {
        String::new()
    } else {
        let raw = line.strip_prefix("@@ ").unwrap_or(line);
        raw.strip_suffix(" @@").unwrap_or(raw).trim().to_string()
    }
}

/// Parses a v4a format patch string into a list of patch operations.
///
/// # Errors
/// Returns an error if the patch format is invalid.
pub fn parse_v4a_patch(text: &str) -> Result<Vec<PatchOperation>, String> {
    let mut lines: Vec<&str> = text.lines().collect();
    let mut ops = Vec::new();
    let mut i = 0;

    // Strip heredoc wrapper
    if let Some(first) = lines.first() {
        let trimmed = first.trim();
        if trimmed == "<<EOF" || trimmed == "<<'EOF'" || trimmed == "<<\"EOF\"" {
            if lines.last().map(|l| l.trim()) == Some("EOF") {
                lines = lines[1..lines.len() - 1].to_vec();
            }
        }
    }

    // Find "*** Begin Patch"
    while i < lines.len() {
        if lines[i].trim() == "*** Begin Patch" {
            i += 1;
            break;
        }
        i += 1;
    }

    while i < lines.len() {
        let line = lines[i].trim();

        if line == "*** End Patch" {
            break;
        }

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            let path = path.to_string();
            i += 1;
            let mut content = String::new();
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("*** ") {
                    break;
                }
                if let Some(text_line) = l.strip_prefix('+') {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(text_line);
                } else {
                    return Err(format!("Expected '+' prefix in Add File block, got: {l}"));
                }
                i += 1;
            }
            ops.push(PatchOperation::Add { path, content });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(PatchOperation::Delete {
                path: path.to_string(),
            });
            i += 1;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            let path = path.to_string();
            i += 1;

            // Check for *** Move to:
            let new_path = if i < lines.len() {
                if let Some(np) = lines[i].trim().strip_prefix("*** Move to: ") {
                    i += 1;
                    Some(np.to_string())
                } else {
                    None
                }
            } else {
                None
            };

            let mut hunks = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("*** ") && !is_hunk_start(l) {
                    break;
                }
                if is_hunk_start(l) {
                    // Consume stacked @@ lines, keeping the last context
                    let mut context_line = extract_context_line(l);
                    i += 1;
                    while i < lines.len() && is_hunk_start(lines[i]) {
                        context_line = extract_context_line(lines[i]);
                        i += 1;
                    }

                    let mut changes = Vec::new();
                    while i < lines.len() {
                        let cl = lines[i];
                        if cl.starts_with("*** ") || is_hunk_start(cl) {
                            break;
                        }
                        if let Some(removed) = cl.strip_prefix('-') {
                            changes.push(Change::Remove(removed.to_string()));
                        } else if let Some(added) = cl.strip_prefix('+') {
                            changes.push(Change::Add(added.to_string()));
                        } else if let Some(ctx) = cl.strip_prefix(' ') {
                            changes.push(Change::Context(ctx.to_string()));
                        } else if cl.is_empty() {
                            changes.push(Change::Context(String::new()));
                        } else {
                            return Err(format!(
                                "Unexpected line in hunk (expected +, -, or space prefix): {cl}"
                            ));
                        }
                        i += 1;
                    }

                    // Check for *** End of File marker
                    let end_of_file =
                        if i < lines.len() && lines[i].trim() == "*** End of File" {
                            i += 1;
                            true
                        } else {
                            false
                        };

                    hunks.push(Hunk {
                        context_line,
                        changes,
                        end_of_file,
                    });
                } else {
                    return Err(format!("Expected @@ context line, got: {l}"));
                }
            }
            ops.push(PatchOperation::Update {
                path,
                new_path,
                hunks,
            });
        } else {
            return Err(format!("Unexpected line in patch: {line}"));
        }
    }

    Ok(ops)
}

/// Applies a list of patch operations using the given sandbox.
///
/// # Errors
/// Returns an error if any file operation fails.
pub async fn apply_patch_operations(
    ops: &[PatchOperation],
    env: &dyn Sandbox,
) -> Result<String, String> {
    let mut results = Vec::new();

    for op in ops {
        match op {
            PatchOperation::Add { path, content } => {
                env.write_file(path, content).await?;
                results.push(format!("Added file: {path}"));
            }
            PatchOperation::Delete { path } => {
                env.delete_file(path).await?;
                results.push(format!("Deleted file: {path}"));
            }
            PatchOperation::Update {
                path,
                new_path,
                hunks,
            } => {
                let original = env.read_file(path, None, None).await?;
                let updated = apply_hunks(&original, hunks)
                    .map_err(|err| format_patch_error(&err, path, &original))?;
                let dest = new_path.as_deref().unwrap_or(path);
                env.write_file(dest, &updated).await?;
                if new_path.is_some() {
                    env.delete_file(path).await?;
                    results.push(format!("Moved file: {path} → {dest}"));
                } else {
                    results.push(format!("Updated file: {path}"));
                }
            }
        }
    }

    Ok(results.join("\n"))
}

fn normalize_char(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
        | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2007}' | '\u{202F}' => ' ',
        other => other,
    }
}

fn normalize_unicode(s: &str) -> String {
    s.chars().map(normalize_char).collect()
}

fn find_line_match(lines: &[String], target: &str, start: usize) -> Option<usize> {
    let slice = &lines[start..];

    // Pass 1: exact
    if let Some(pos) = slice.iter().position(|l| l.as_str() == target) {
        return Some(start + pos);
    }
    // Pass 2: trim_end
    let target_te = target.trim_end();
    if let Some(pos) = slice.iter().position(|l| l.trim_end() == target_te) {
        return Some(start + pos);
    }
    // Pass 3: trim
    let target_t = target.trim();
    if let Some(pos) = slice.iter().position(|l| l.trim() == target_t) {
        return Some(start + pos);
    }
    // Pass 4: unicode normalization
    let target_n = normalize_unicode(target_t);
    slice
        .iter()
        .position(|l| normalize_unicode(l.trim()) == target_n)
        .map(|p| start + p)
}

fn find_line_match_reverse(lines: &[String], target: &str) -> Option<usize> {
    // Pass 1: exact
    if let Some(pos) = lines.iter().rposition(|l| l.as_str() == target) {
        return Some(pos);
    }
    // Pass 2: trim_end
    let target_te = target.trim_end();
    if let Some(pos) = lines.iter().rposition(|l| l.trim_end() == target_te) {
        return Some(pos);
    }
    // Pass 3: trim
    let target_t = target.trim();
    if let Some(pos) = lines.iter().rposition(|l| l.trim() == target_t) {
        return Some(pos);
    }
    // Pass 4: unicode normalization
    let target_n = normalize_unicode(target_t);
    lines
        .iter()
        .rposition(|l| normalize_unicode(l.trim()) == target_n)
}

fn apply_hunks(content: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut cursor = 0;

    for hunk in hunks {
        let has_explicit_context = !hunk.context_line.is_empty();

        let context_pos = if hunk.end_of_file {
            if has_explicit_context {
                find_line_match_reverse(&lines, &hunk.context_line)
            } else {
                let first_match_text = hunk
                    .changes
                    .iter()
                    .find_map(|c| match c {
                        Change::Remove(t) | Change::Context(t) => Some(t.as_str()),
                        Change::Add(_) => None,
                    })
                    .ok_or(
                        "Hunk with bare @@ has no remove or context lines to locate position",
                    )?;
                find_line_match_reverse(&lines, first_match_text)
            }
            .ok_or_else(|| {
                let target = if has_explicit_context {
                    &hunk.context_line
                } else {
                    "first change line"
                };
                format!("Could not find context line in file: '{target}'")
            })?
        } else if has_explicit_context {
            find_line_match(&lines, &hunk.context_line, cursor).ok_or_else(|| {
                format!(
                    "Could not find context line in file: '{}'",
                    hunk.context_line
                )
            })?
        } else {
            // Bare @@ — locate position from the first remove or context line
            let first_match_text = hunk
                .changes
                .iter()
                .find_map(|c| match c {
                    Change::Remove(t) | Change::Context(t) => Some(t.as_str()),
                    Change::Add(_) => None,
                })
                .ok_or("Hunk with bare @@ has no remove or context lines to locate position")?;
            find_line_match(&lines, first_match_text, cursor)
                .ok_or_else(|| format!("Could not find line in file: '{first_match_text}'"))?
        };

        // Build what we expect to find and what to replace with
        let mut new_lines: Vec<String> = Vec::new();

        let mut file_idx = context_pos;
        if has_explicit_context {
            // The context line itself is preserved
            new_lines.push(lines[context_pos].clone());
            file_idx = context_pos + 1;
        }

        for change in &hunk.changes {
            match change {
                Change::Remove(_) => {
                    file_idx += 1;
                }
                Change::Add(text) => {
                    new_lines.push(text.clone());
                }
                Change::Context(_) => {
                    if file_idx < lines.len() {
                        new_lines.push(lines[file_idx].clone());
                    }
                    file_idx += 1;
                }
            }
        }

        // Calculate total lines consumed from original
        let explicit_context_count = if has_explicit_context { 1 } else { 0 };
        let total_original_lines = explicit_context_count
            + hunk
                .changes
                .iter()
                .filter(|c| matches!(c, Change::Remove(_) | Change::Context(_)))
                .count();

        // Replace the range
        let end = (context_pos + total_original_lines).min(lines.len());
        let replacement_len = new_lines.len();
        lines.splice(context_pos..end, new_lines);
        cursor = context_pos + replacement_len;
    }

    Ok(lines.join("\n"))
}

fn format_patch_error(error: &str, path: &str, content: &str) -> String {
    let numbered = format_lines_numbered(content, None, None);
    let truncated = truncate_output(&numbered, 9_000, TruncationMode::HeadTail);
    format!("{error}\n\nCurrent contents of {path}:\n{truncated}")
}

pub fn make_apply_patch_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "apply_patch".into(),
            description: "Apply a v4a format patch to modify files".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "The patch content in v4a format"
                    }
                },
                "required": ["patch"]
            }),
        },
        executor: Arc::new(|args, ctx| {
            Box::pin(async move {
                let patch_text = args
                    .get("patch")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: patch".to_string())?;

                let ops = parse_v4a_patch(patch_text)?;
                apply_patch_operations(&ops, ctx.env.as_ref()).await
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MutableMockSandbox;
    use std::collections::HashMap;

    #[test]
    fn parse_v4a_add_file() {
        let patch = "\
*** Begin Patch
*** Add File: src/new_file.rs
+fn main() {
+    println!(\"hello\");
+}
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            PatchOperation::Add {
                path: "src/new_file.rs".into(),
                content: "fn main() {\n    println!(\"hello\");\n}".into(),
            }
        );
    }

    #[test]
    fn parse_v4a_delete_file() {
        let patch = "\
*** Begin Patch
*** Delete File: src/old_file.rs
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            PatchOperation::Delete {
                path: "src/old_file.rs".into(),
            }
        );
    }

    #[test]
    fn parse_v4a_update_file() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.rs
@@ fn hello() @@
-    println!(\"old\");
+    println!(\"new\");
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::Update {
                path,
                new_path,
                hunks,
            } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(*new_path, None);
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].context_line, "fn hello()");
                assert!(!hunks[0].end_of_file);
                assert_eq!(hunks[0].changes.len(), 2);
                assert_eq!(
                    hunks[0].changes[0],
                    Change::Remove("    println!(\"old\");".into())
                );
                assert_eq!(
                    hunks[0].changes[1],
                    Change::Add("    println!(\"new\");".into())
                );
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn parse_v4a_multi_operation() {
        let patch = "\
*** Begin Patch
*** Add File: src/a.rs
+// file a
*** Delete File: src/b.rs
*** Update File: src/c.rs
@@ fn main() @@
-    old_call();
+    new_call();
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 3);
        assert!(matches!(&ops[0], PatchOperation::Add { .. }));
        assert!(matches!(&ops[1], PatchOperation::Delete { .. }));
        assert!(matches!(&ops[2], PatchOperation::Update { .. }));
    }

    #[test]
    fn parse_v4a_bare_at_at_hunk() {
        let patch = "\
*** Begin Patch
*** Update File: src/game.py
@@
-from src.cards import Suit
+from src.cards import Card, Suit
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::Update { path, hunks, .. } => {
                assert_eq!(path, "src/game.py");
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].context_line, "");
                assert_eq!(hunks[0].changes.len(), 2);
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn parse_v4a_multiple_bare_at_at_hunks() {
        let patch = "\
*** Begin Patch
*** Update File: src/game.py
@@
-from src.cards import Suit
+from src.cards import Card, Suit
@@
-    stock: list = field(default_factory=list)
+    stock: list[Card] = field(default_factory=list)
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks.len(), 2);
                assert_eq!(hunks[0].context_line, "");
                assert_eq!(hunks[1].context_line, "");
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[tokio::test]
    async fn apply_patch_bare_at_at_update() {
        let mut files = HashMap::new();
        files.insert(
            "src/game.py".to_string(),
            "from src.cards import Suit\nfrom src.piles import Pile\n\nclass GameState:\n    stock: list = field(default_factory=list)\n    waste: list = field(default_factory=list)".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/game.py".into(),
            new_path: None,
            hunks: vec![
                Hunk {
                    context_line: String::new(),
                    end_of_file: false,
                    changes: vec![
                        Change::Remove("from src.cards import Suit".into()),
                        Change::Add("from src.cards import Card, Suit".into()),
                    ],
                },
                Hunk {
                    context_line: String::new(),
                    end_of_file: false,
                    changes: vec![
                        Change::Remove("    stock: list = field(default_factory=list)".into()),
                        Change::Remove("    waste: list = field(default_factory=list)".into()),
                        Change::Add("    stock: list[Card] = field(default_factory=list)".into()),
                        Change::Add("    waste: list[Card] = field(default_factory=list)".into()),
                    ],
                },
            ],
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Updated file: src/game.py"));

        let content = env.read_file("src/game.py", None, None).await.unwrap();
        assert!(content.contains("from src.cards import Card, Suit"));
        assert!(!content.contains("from src.cards import Suit\n"));
        assert!(content.contains("stock: list[Card]"));
        assert!(content.contains("waste: list[Card]"));
        assert!(content.contains("from src.piles import Pile"));
    }

    #[test]
    fn parse_v4a_mixed_bare_and_contextual_hunks() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.rs
@@ fn setup() @@
-    old_setup();
+    new_setup();
@@
-    old_teardown();
+    new_teardown();
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks.len(), 2);
                assert_eq!(hunks[0].context_line, "fn setup()");
                assert_eq!(hunks[1].context_line, "");
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn parse_v4a_bare_at_at_with_context_lines() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.rs
@@
 fn unchanged() {
-    old_line();
+    new_line();
 }
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].context_line, "");
                assert_eq!(hunks[0].changes.len(), 4);
                assert_eq!(hunks[0].changes[0], Change::Context("fn unchanged() {".into()));
                assert_eq!(hunks[0].changes[1], Change::Remove("    old_line();".into()));
                assert_eq!(hunks[0].changes[2], Change::Add("    new_line();".into()));
                assert_eq!(hunks[0].changes[3], Change::Context("}".into()));
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn parse_v4a_bare_at_at_add_only_errors_on_apply() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.rs
@@
+new_line();
*** End Patch";

        // Parsing succeeds — the hunk is structurally valid
        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks[0].context_line, "");
                assert_eq!(hunks[0].changes.len(), 1);
                assert_eq!(hunks[0].changes[0], Change::Add("new_line();".into()));
            }
            _ => panic!("Expected Update operation"),
        }

        // Applying fails — no remove/context line to locate position
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                let result = apply_hunks("fn main() {}\n", hunks);
                assert!(result.is_err());
                assert!(result.unwrap_err().contains("no remove or context lines"));
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[tokio::test]
    async fn apply_patch_bare_at_at_with_context_lines() {
        let mut files = HashMap::new();
        files.insert(
            "src/lib.rs".to_string(),
            "fn unchanged() {\n    old_line();\n}".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/lib.rs".into(),
            new_path: None,
            hunks: vec![Hunk {
                context_line: String::new(),
                end_of_file: false,
                changes: vec![
                    Change::Context("fn unchanged() {".into()),
                    Change::Remove("    old_line();".into()),
                    Change::Add("    new_line();".into()),
                    Change::Context("}".into()),
                ],
            }],
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Updated file: src/lib.rs"));

        let content = env.read_file("src/lib.rs", None, None).await.unwrap();
        assert_eq!(content, "fn unchanged() {\n    new_line();\n}");
    }

    #[tokio::test]
    async fn apply_patch_mixed_bare_and_contextual_hunks() {
        let mut files = HashMap::new();
        files.insert(
            "src/lib.rs".to_string(),
            "import foo\nimport bar\n\ndef setup():\n    old_setup()\n\ndef teardown():\n    old_teardown()\n".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/lib.rs".into(),
            new_path: None,
            hunks: vec![
                Hunk {
                    context_line: "def setup():".into(),
                    end_of_file: false,
                    changes: vec![
                        Change::Remove("    old_setup()".into()),
                        Change::Add("    new_setup()".into()),
                    ],
                },
                Hunk {
                    context_line: String::new(),
                    end_of_file: false,
                    changes: vec![
                        Change::Remove("    old_teardown()".into()),
                        Change::Add("    new_teardown()".into()),
                    ],
                },
            ],
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Updated file: src/lib.rs"));

        let content = env.read_file("src/lib.rs", None, None).await.unwrap();
        assert!(content.contains("new_setup()"));
        assert!(content.contains("new_teardown()"));
        assert!(!content.contains("old_setup()"));
        assert!(!content.contains("old_teardown()"));
    }

    #[tokio::test]
    async fn apply_patch_add_file() {
        let env = MutableMockSandbox::new(HashMap::new());
        let ops = vec![PatchOperation::Add {
            path: "src/new.rs".into(),
            content: "fn new() {}".into(),
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Added file: src/new.rs"));

        let content = env.read_file("src/new.rs", None, None).await.unwrap();
        assert_eq!(content, "fn new() {}");
    }

    #[tokio::test]
    async fn apply_patch_update_file() {
        let mut files = HashMap::new();
        files.insert(
            "src/lib.rs".to_string(),
            "fn hello() {\n    println!(\"old\");\n}".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/lib.rs".into(),
            new_path: None,
            hunks: vec![Hunk {
                context_line: "fn hello() {".into(),
                end_of_file: false,
                changes: vec![
                    Change::Remove("    println!(\"old\");".into()),
                    Change::Add("    println!(\"new\");".into()),
                ],
            }],
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Updated file: src/lib.rs"));

        let content = env.read_file("src/lib.rs", None, None).await.unwrap();
        assert!(content.contains("println!(\"new\")"));
        assert!(!content.contains("println!(\"old\")"));
    }

    #[test]
    fn format_patch_error_includes_numbered_contents() {
        let result = format_patch_error(
            "Could not find context line in file: 'fn missing()'",
            "src/lib.rs",
            "fn hello() {\n    println!(\"hi\");\n}",
        );
        assert!(result.contains("Could not find context line in file: 'fn missing()'"));
        assert!(result.contains("Current contents of src/lib.rs:"));
        assert!(result.contains("1 | fn hello() {"));
        assert!(result.contains("2 |     println!(\"hi\");"));
        assert!(result.contains("3 | }"));
    }

    #[test]
    fn format_patch_error_truncates_large_files() {
        let lines: Vec<String> = (1..=1_000)
            .map(|i| format!("line number {:04}", i))
            .collect();
        let content = lines.join("\n");
        let result = format_patch_error("some error", "big.txt", &content);
        assert!(result.len() < 10_000);
        assert!(result.contains("truncated") || result.contains("removed"));
    }

    #[tokio::test]
    async fn apply_patch_error_includes_file_contents() {
        let mut files = HashMap::new();
        files.insert(
            "src/game.py".to_string(),
            "def real_fn():\n    pass".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/game.py".into(),
            new_path: None,
            hunks: vec![Hunk {
                context_line: "def nonexistent():".into(),
                end_of_file: false,
                changes: vec![
                    Change::Remove("    old_body()".into()),
                    Change::Add("    new_body()".into()),
                ],
            }],
        }];

        let err = apply_patch_operations(&ops, &env).await.unwrap_err();
        assert!(err.contains("Could not find context line"));
        assert!(err.contains("Current contents of src/game.py:"));
        assert!(err.contains("1 | def real_fn():"));
    }

    // Phase 0: Forward-order hunk application

    #[test]
    fn apply_hunks_bare_at_at_searches_forward_from_previous_hunk() {
        let content = "def foo():\n    pass\n\ndef bar():\n    pass";
        let hunks = vec![
            Hunk {
                context_line: String::new(),
                end_of_file: false,
                changes: vec![
                    Change::Remove("    pass".into()),
                    Change::Add("    return 1".into()),
                ],
            },
            Hunk {
                context_line: String::new(),
                end_of_file: false,
                changes: vec![
                    Change::Remove("    pass".into()),
                    Change::Add("    return 2".into()),
                ],
            },
        ];
        let result = apply_hunks(content, &hunks).unwrap();
        assert!(result.contains("return 1"));
        assert!(result.contains("return 2"));
        assert!(!result.contains("    pass"));
    }

    // Phase 1: Context without trailing @@

    #[test]
    fn parse_v4a_context_without_trailing_markers() {
        let patch = "\
*** Begin Patch
*** Update File: src/hello.py
@@ def hello():
-    print(\"old\")
+    print(\"new\")
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks[0].context_line, "def hello():");
            }
            _ => panic!("Expected Update operation"),
        }
    }

    // Phase 2: Stacked @@ anchors

    #[test]
    fn parse_v4a_stacked_context_uses_last() {
        let patch = "\
*** Begin Patch
*** Update File: src/foo.py
@@ class Foo:
@@   def bar(self):
-        pass
+        return 42
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].context_line, "def bar(self):");
            }
            _ => panic!("Expected Update operation"),
        }
    }

    // Phase 3: *** End of File

    #[test]
    fn parse_v4a_end_of_file_marker() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.py
@@
-    pass
+    return 1
*** End of File
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].end_of_file);
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn apply_hunks_end_of_file_searches_backward() {
        // Two functions with identical "pass" line — End of File matches the last one
        let content = "def foo():\n    pass\n\ndef bar():\n    pass";
        let hunks = vec![Hunk {
            context_line: String::new(),
            end_of_file: true,
            changes: vec![
                Change::Remove("    pass".into()),
                Change::Add("    return 99".into()),
            ],
        }];
        let result = apply_hunks(content, &hunks).unwrap();
        // First "pass" should be untouched, second should be replaced
        assert_eq!(
            result,
            "def foo():\n    pass\n\ndef bar():\n    return 99"
        );
    }

    // Phase 4: *** Move to:

    #[test]
    fn parse_v4a_move_to() {
        let patch = "\
*** Begin Patch
*** Update File: src/old.py
*** Move to: src/new.py
@@ def hello():
-    pass
+    return 1
*** End Patch";

        let ops = parse_v4a_patch(patch).unwrap();
        match &ops[0] {
            PatchOperation::Update {
                path,
                new_path,
                hunks,
            } => {
                assert_eq!(path, "src/old.py");
                assert_eq!(*new_path, Some("src/new.py".to_string()));
                assert_eq!(hunks.len(), 1);
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[tokio::test]
    async fn apply_patch_move_to_renames_file() {
        let mut files = HashMap::new();
        files.insert(
            "src/old.py".to_string(),
            "def hello():\n    pass".to_string(),
        );
        let env = MutableMockSandbox::new(files);

        let ops = vec![PatchOperation::Update {
            path: "src/old.py".into(),
            new_path: Some("src/new.py".into()),
            hunks: vec![Hunk {
                context_line: "def hello():".into(),
                end_of_file: false,
                changes: vec![
                    Change::Remove("    pass".into()),
                    Change::Add("    return 1".into()),
                ],
            }],
        }];

        let result = apply_patch_operations(&ops, &env).await.unwrap();
        assert!(result.contains("Moved file"));

        // New path exists with updated content
        let content = env.read_file("src/new.py", None, None).await.unwrap();
        assert_eq!(content, "def hello():\n    return 1");

        // Old path is deleted
        let old = env.read_file("src/old.py", None, None).await;
        assert!(old.is_err());
    }

    // Phase 5: Fuzzy matching

    #[test]
    fn apply_hunks_prefers_exact_match_over_trimmed() {
        // Line 0 has leading spaces, line 1 is exact match
        let content = "  indented\nindented";
        let hunks = vec![Hunk {
            context_line: "indented".into(),
            end_of_file: false,
            changes: vec![Change::Add("extra".into())],
        }];
        let result = apply_hunks(content, &hunks).unwrap();
        // Should match line 1 (exact), so "extra" inserted after "indented" (line 1)
        assert_eq!(result, "  indented\nindented\nextra");
    }

    #[test]
    fn apply_hunks_fuzzy_unicode_normalization() {
        let content = "print(\u{201C}hello\u{201D})";
        let hunks = vec![Hunk {
            context_line: "print(\"hello\")".into(),
            end_of_file: false,
            changes: vec![Change::Add("print(\"world\")".into())],
        }];
        let result = apply_hunks(content, &hunks).unwrap();
        // Original line preserved, new line added after
        assert!(result.contains("print(\u{201C}hello\u{201D})"));
        assert!(result.contains("print(\"world\")"));
    }

    // Phase 6: Heredoc stripping

    #[test]
    fn parse_v4a_strips_heredoc_wrapper() {
        let patch = "\
<<'EOF'
*** Begin Patch
*** Update File: src/lib.rs
@@ fn hello():
-    pass
+    return 1
*** End Patch
EOF";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::Update { hunks, .. } => {
                assert_eq!(hunks[0].context_line, "fn hello():");
            }
            _ => panic!("Expected Update operation"),
        }
    }

    #[test]
    fn parse_v4a_strips_heredoc_unquoted() {
        let patch = "\
<<EOF
*** Begin Patch
*** Add File: src/a.rs
+// hello
*** End Patch
EOF";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], PatchOperation::Add { .. }));
    }

    #[test]
    fn parse_v4a_strips_heredoc_double_quoted() {
        let patch = "\
<<\"EOF\"
*** Begin Patch
*** Delete File: src/old.rs
*** End Patch
EOF";

        let ops = parse_v4a_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], PatchOperation::Delete { .. }));
    }
}
