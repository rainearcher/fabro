use crate::execution_env::ExecutionEnvironment;

const BUDGET_BYTES: usize = 32768;

pub async fn discover_project_docs(
    env: &dyn ExecutionEnvironment,
    git_root: &str,
    working_dir: &str,
    provider_id: &str,
) -> Vec<String> {
    let directories = build_directory_walk(git_root, working_dir);

    let candidate_filenames: Vec<&str> = match provider_id {
        "anthropic" => vec!["AGENTS.md", "CLAUDE.md"],
        "openai" => vec!["AGENTS.md", ".github/copilot-instructions.md"],
        "gemini" => vec!["AGENTS.md", "GEMINI.md"],
        _ => vec!["AGENTS.md"],
    };

    let mut results = Vec::new();
    let mut budget_remaining = BUDGET_BYTES;

    for dir in &directories {
        for filename in &candidate_filenames {
            let path = format!("{dir}/{filename}");
            if let Ok(content) = env.read_file(&path).await {
                if content.is_empty() {
                    continue;
                }
                if content.len() <= budget_remaining {
                    budget_remaining -= content.len();
                    results.push(content);
                } else if budget_remaining > 0 {
                    let truncated = truncate_to_budget(&content, budget_remaining);
                    budget_remaining = 0;
                    results.push(truncated);
                }
            }
        }
    }

    results
}

fn build_directory_walk(git_root: &str, working_dir: &str) -> Vec<String> {
    let mut dirs = vec![git_root.to_string()];

    if working_dir == git_root {
        return dirs;
    }

    // Strip git_root prefix to get relative path components
    let relative = working_dir
        .strip_prefix(git_root)
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or("");

    if relative.is_empty() {
        return dirs;
    }

    let mut current = git_root.to_string();
    let parts: Vec<&str> = relative.split('/').collect();
    for part in parts {
        current = format!("{current}/{part}");
        dirs.push(current.clone());
    }

    dirs
}

fn truncate_to_budget(content: &str, budget: usize) -> String {
    const MARKER: &str = "... [truncated]";
    if budget <= MARKER.len() {
        return MARKER[..budget].to_string();
    }
    let usable = budget - MARKER.len();
    // Find the last valid char boundary within usable bytes
    let mut end = usable;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &content[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct DocEnv {
        files: HashMap<String, String>,
    }

    #[async_trait]
    impl ExecutionEnvironment for DocEnv {
        async fn read_file(&self, path: &str) -> Result<String, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("not found: {path}"))
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, path: &str) -> Result<bool, String> {
            Ok(self.files.contains_key(path))
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn initialize(&self) -> Result<(), String> {
            Ok(())
        }
        async fn cleanup(&self) -> Result<(), String> {
            Ok(())
        }
        fn working_directory(&self) -> &str {
            "/tmp"
        }
        fn platform(&self) -> &str {
            "darwin"
        }
        fn os_version(&self) -> String {
            String::new()
        }
    }

    #[tokio::test]
    async fn discovers_agents_md() {
        let mut files = HashMap::new();
        files.insert("/repo/AGENTS.md".into(), "Agent instructions".into());
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv { files });
        let docs = discover_project_docs(env.as_ref(), "/repo", "/repo", "anthropic").await;
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0], "Agent instructions");
    }

    #[tokio::test]
    async fn filters_by_provider() {
        let mut files = HashMap::new();
        files.insert("/repo/AGENTS.md".into(), "agents".into());
        files.insert("/repo/CLAUDE.md".into(), "claude".into());
        files.insert(
            "/repo/.github/copilot-instructions.md".into(),
            "copilot".into(),
        );
        files.insert("/repo/GEMINI.md".into(), "gemini".into());

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv {
            files: files.clone(),
        });
        let anthropic_docs =
            discover_project_docs(env.as_ref(), "/repo", "/repo", "anthropic").await;
        assert_eq!(anthropic_docs.len(), 2);
        assert_eq!(anthropic_docs[0], "agents");
        assert_eq!(anthropic_docs[1], "claude");

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv {
            files: files.clone(),
        });
        let openai_docs = discover_project_docs(env.as_ref(), "/repo", "/repo", "openai").await;
        assert_eq!(openai_docs.len(), 2);
        assert_eq!(openai_docs[0], "agents");
        assert_eq!(openai_docs[1], "copilot");

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv { files });
        let gemini_docs = discover_project_docs(env.as_ref(), "/repo", "/repo", "gemini").await;
        assert_eq!(gemini_docs.len(), 2);
        assert_eq!(gemini_docs[0], "agents");
        assert_eq!(gemini_docs[1], "gemini");
    }

    #[tokio::test]
    async fn truncates_at_budget() {
        let mut files = HashMap::new();
        // Create content that exceeds 32KB budget
        let large_content = "x".repeat(30000);
        let second_content = "y".repeat(5000);
        files.insert("/repo/AGENTS.md".into(), large_content.clone());
        files.insert("/repo/CLAUDE.md".into(), second_content);

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv { files });
        let docs = discover_project_docs(env.as_ref(), "/repo", "/repo", "anthropic").await;
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0], large_content);
        // Second doc should be truncated to fit remaining budget
        assert!(docs[1].ends_with("... [truncated]"));
        assert!(docs[0].len() + docs[1].len() <= BUDGET_BYTES);
    }

    #[tokio::test]
    async fn walks_directory_hierarchy() {
        let mut files = HashMap::new();
        files.insert("/repo/AGENTS.md".into(), "root agents".into());
        files.insert("/repo/src/AGENTS.md".into(), "src agents".into());
        files.insert("/repo/src/app/AGENTS.md".into(), "app agents".into());

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DocEnv { files });
        let docs =
            discover_project_docs(env.as_ref(), "/repo", "/repo/src/app", "anthropic").await;
        assert_eq!(docs.len(), 3);
        assert_eq!(docs[0], "root agents");
        assert_eq!(docs[1], "src agents");
        assert_eq!(docs[2], "app agents");
    }
}
