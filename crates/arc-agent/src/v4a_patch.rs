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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOperation {
    Add { path: String, content: String },
    Delete { path: String },
    Update { path: String, hunks: Vec<Hunk> },
}

/// Parses a v4a format patch string into a list of patch operations.
///
/// # Errors
/// Returns an error if the patch format is invalid.
pub fn parse_v4a_patch(text: &str) -> Result<Vec<PatchOperation>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut ops = Vec::new();
    let mut i = 0;

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
            let mut hunks = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("*** ") && !l.starts_with("@@ ") {
                    break;
                }
                if l == "@@" || (l.starts_with("@@ ") && l.ends_with(" @@")) {
                    let context_line = if l == "@@" {
                        String::new()
                    } else {
                        l[3..l.len() - 3].to_string()
                    };
                    i += 1;
                    let mut changes = Vec::new();
                    while i < lines.len() {
                        let cl = lines[i];
                        if cl.starts_with("*** ")
                            || cl == "@@"
                            || (cl.starts_with("@@ ") && cl.ends_with(" @@"))
                        {
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
                    hunks.push(Hunk {
                        context_line,
                        changes,
                    });
                } else {
                    return Err(format!("Expected @@ context @@ line, got: {l}"));
                }
            }
            ops.push(PatchOperation::Update { path, hunks });
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
            PatchOperation::Update { path, hunks } => {
                let original = env.read_file(path, None, None).await?;
                let updated = apply_hunks(&original, hunks)
                    .map_err(|err| format_patch_error(&err, path, &original))?;
                env.write_file(path, &updated).await?;
                results.push(format!("Updated file: {path}"));
            }
        }
    }

    Ok(results.join("\n"))
}

fn apply_hunks(content: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    // Apply hunks in reverse order to preserve line indices
    for hunk in hunks.iter().rev() {
        let has_explicit_context = !hunk.context_line.is_empty();

        let context_pos = if has_explicit_context {
            lines
                .iter()
                .position(|l| l.trim() == hunk.context_line.trim())
                .ok_or_else(|| {
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
            lines
                .iter()
                .position(|l| l.trim() == first_match_text.trim())
                .ok_or_else(|| {
                    format!("Could not find line in file: '{first_match_text}'")
                })?
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
        lines.splice(context_pos..end, new_lines);
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
            PatchOperation::Update { path, hunks } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].context_line, "fn hello()");
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
            PatchOperation::Update { path, hunks } => {
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
            hunks: vec![
                Hunk {
                    context_line: String::new(),
                    changes: vec![
                        Change::Remove("from src.cards import Suit".into()),
                        Change::Add("from src.cards import Card, Suit".into()),
                    ],
                },
                Hunk {
                    context_line: String::new(),
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
            hunks: vec![Hunk {
                context_line: String::new(),
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
            hunks: vec![
                Hunk {
                    context_line: "def setup():".into(),
                    changes: vec![
                        Change::Remove("    old_setup()".into()),
                        Change::Add("    new_setup()".into()),
                    ],
                },
                Hunk {
                    context_line: String::new(),
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
            hunks: vec![Hunk {
                context_line: "fn hello() {".into(),
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
            hunks: vec![Hunk {
                context_line: "def nonexistent():".into(),
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
}
