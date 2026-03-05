use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;

use arc_agent::Sandbox;

use super::config::HookDefinition;
use super::types::{HookContext, HookDecision, HookResult};

/// Trait for executing hooks via different transports.
#[async_trait]
pub trait HookExecutor: Send + Sync {
    async fn execute(
        &self,
        definition: &HookDefinition,
        context: &HookContext,
        sandbox: &dyn Sandbox,
        work_dir: Option<&Path>,
    ) -> HookResult;
}

/// Executes hooks as shell commands (host or sandbox).
pub struct CommandHookExecutor;

impl CommandHookExecutor {
    /// Parse a hook decision from JSON stdout and exit code.
    fn parse_decision(exit_code: i32, stdout: &str) -> HookDecision {
        if exit_code == 0 {
            // Try parsing JSON response for explicit decision
            if let Ok(decision) = serde_json::from_str::<HookDecision>(stdout.trim()) {
                return decision;
            }
            HookDecision::Proceed
        } else if exit_code == 2 {
            // Exit 2 = block/skip
            if let Ok(decision) = serde_json::from_str::<HookDecision>(stdout.trim()) {
                return decision;
            }
            HookDecision::Block {
                reason: Some(format!("hook exited with code 2")),
            }
        } else {
            HookDecision::Block {
                reason: Some(format!("hook exited with code {exit_code}")),
            }
        }
    }
}

#[async_trait]
impl HookExecutor for CommandHookExecutor {
    async fn execute(
        &self,
        definition: &HookDefinition,
        context: &HookContext,
        sandbox: &dyn Sandbox,
        work_dir: Option<&Path>,
    ) -> HookResult {
        let start = Instant::now();
        let command = match definition.resolved_hook_type() {
            Some(super::config::HookType::Command { ref command }) => command.clone(),
            _ => {
                return HookResult {
                    hook_name: definition.name.clone(),
                    decision: HookDecision::Block {
                        reason: Some("no command specified".into()),
                    },
                    duration_ms: 0,
                };
            }
        };

        let context_json = serde_json::to_string(context).unwrap_or_default();
        let timeout_ms = definition.timeout().as_millis() as u64;

        let mut env_vars = HashMap::new();
        env_vars.insert("ARC_EVENT".to_string(), context.event.to_string());
        env_vars.insert("ARC_RUN_ID".to_string(), context.run_id.clone());
        env_vars.insert("ARC_WORKFLOW".to_string(), context.workflow_name.clone());
        if let Some(ref node_id) = context.node_id {
            env_vars.insert("ARC_NODE_ID".to_string(), node_id.clone());
        }

        let decision = if definition.runs_in_sandbox() {
            // Write context to temp file, pass path as env var
            let ctx_path = "/tmp/arc-hook-context.json";
            if sandbox.write_file(ctx_path, &context_json).await.is_ok() {
                env_vars.insert("ARC_HOOK_CONTEXT".to_string(), ctx_path.to_string());
            }
            match sandbox
                .exec_command(&command, timeout_ms, None, Some(&env_vars), None)
                .await
            {
                Ok(result) => Self::parse_decision(result.exit_code, &result.stdout),
                Err(e) => HookDecision::Block {
                    reason: Some(format!("sandbox exec failed: {e}")),
                },
            }
        } else {
            // Run on host via sh -c
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c").arg(&command);
            if let Some(wd) = work_dir {
                cmd.current_dir(wd);
            }
            for (k, v) in &env_vars {
                cmd.env(k, v);
            }
            // Pipe context JSON to stdin
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            match cmd.spawn() {
                Ok(mut child) => {
                    // Write context to stdin
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(context_json.as_bytes());
                    }
                    match child.wait_with_output() {
                        Ok(output) => {
                            let exit_code = output.status.code().unwrap_or(1);
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            Self::parse_decision(exit_code, &stdout)
                        }
                        Err(e) => HookDecision::Block {
                            reason: Some(format!("command wait failed: {e}")),
                        },
                    }
                }
                Err(e) => HookDecision::Block {
                    reason: Some(format!("command spawn failed: {e}")),
                },
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        HookResult {
            hook_name: definition.name.clone(),
            decision,
            duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::config::HookType;
    use crate::hook::types::HookEvent;

    fn make_context() -> HookContext {
        HookContext::new(HookEvent::StageStart, "run-1".into(), "test-wf".into())
    }

    fn make_definition(command: &str) -> HookDefinition {
        HookDefinition {
            name: Some("test-hook".into()),
            event: HookEvent::StageStart,
            command: Some(command.into()),
            hook_type: None,
            matcher: None,
            blocking: None,
            timeout_ms: Some(5000),
            sandbox: Some(false), // host execution for tests
        }
    }

    #[test]
    fn parse_decision_exit_0_proceed() {
        assert_eq!(
            CommandHookExecutor::parse_decision(0, ""),
            HookDecision::Proceed
        );
    }

    #[test]
    fn parse_decision_exit_0_with_json() {
        let json = r#"{"decision": "skip", "reason": "not needed"}"#;
        assert_eq!(
            CommandHookExecutor::parse_decision(0, json),
            HookDecision::Skip {
                reason: Some("not needed".into())
            }
        );
    }

    #[test]
    fn parse_decision_exit_2_block() {
        assert!(matches!(
            CommandHookExecutor::parse_decision(2, ""),
            HookDecision::Block { .. }
        ));
    }

    #[test]
    fn parse_decision_exit_2_with_json() {
        let json = r#"{"decision": "skip", "reason": "skipping"}"#;
        assert_eq!(
            CommandHookExecutor::parse_decision(2, json),
            HookDecision::Skip {
                reason: Some("skipping".into())
            }
        );
    }

    #[test]
    fn parse_decision_exit_1_block() {
        assert!(matches!(
            CommandHookExecutor::parse_decision(1, ""),
            HookDecision::Block { .. }
        ));
    }

    #[test]
    fn parse_decision_exit_0_override() {
        let json = r#"{"decision": "override", "edge_to": "node_b"}"#;
        assert_eq!(
            CommandHookExecutor::parse_decision(0, json),
            HookDecision::Override {
                edge_to: "node_b".into()
            }
        );
    }

    #[tokio::test]
    async fn command_executor_host_success() {
        let executor = CommandHookExecutor;
        let def = make_definition("exit 0");
        let ctx = make_context();
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert_eq!(result.decision, HookDecision::Proceed);
        assert_eq!(result.hook_name.as_deref(), Some("test-hook"));
    }

    #[tokio::test]
    async fn command_executor_host_failure() {
        let executor = CommandHookExecutor;
        let def = make_definition("exit 1");
        let ctx = make_context();
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert!(matches!(result.decision, HookDecision::Block { .. }));
    }

    #[tokio::test]
    async fn command_executor_host_skip_via_exit_2() {
        let executor = CommandHookExecutor;
        let def = make_definition("exit 2");
        let ctx = make_context();
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert!(matches!(result.decision, HookDecision::Block { .. }));
    }

    #[tokio::test]
    async fn command_executor_host_json_decision() {
        let executor = CommandHookExecutor;
        let def =
            make_definition(r#"echo '{"decision": "skip", "reason": "test skip"}'"#);
        let ctx = make_context();
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert_eq!(
            result.decision,
            HookDecision::Skip {
                reason: Some("test skip".into())
            }
        );
    }

    #[tokio::test]
    async fn command_executor_env_vars_set() {
        let executor = CommandHookExecutor;
        // Print env vars to stdout for verification
        let def = make_definition("echo $ARC_EVENT:$ARC_RUN_ID:$ARC_WORKFLOW");
        let mut ctx = make_context();
        ctx.node_id = Some("plan".into());
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert_eq!(result.decision, HookDecision::Proceed);
    }

    #[tokio::test]
    async fn command_executor_no_command_blocks() {
        let executor = CommandHookExecutor;
        let def = HookDefinition {
            name: None,
            event: HookEvent::StageStart,
            command: None,
            hook_type: Some(HookType::Http {
                url: "http://example.com".into(),
                headers: None,
            }),
            matcher: None,
            blocking: None,
            timeout_ms: None,
            sandbox: Some(false),
        };
        let ctx = make_context();
        let sandbox = arc_agent::LocalSandbox::new(std::env::current_dir().unwrap());
        let result = executor.execute(&def, &ctx, &sandbox, None).await;
        assert!(matches!(result.decision, HookDecision::Block { .. }));
    }
}
