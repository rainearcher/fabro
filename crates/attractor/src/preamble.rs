use std::collections::HashMap;

use crate::context::Context;
use crate::graph::Graph;
use crate::outcome::Outcome;

/// Build a fidelity-appropriate preamble string for non-full context modes.
///
/// The preamble provides prior conversation context to the next LLM session,
/// tailored by the fidelity mode:
/// - `truncate`: Only graph goal and run ID
/// - `compact`: Structured bullet-point summary of completed stages and context
/// - `summary:low`: Brief textual summary (~600 token target)
/// - `summary:medium`: Moderate detail (~1500 token target)
/// - `summary:high`: Detailed summary (~3000 token target)
#[must_use]
pub fn build_preamble(
    fidelity: &str,
    context: &Context,
    graph: &Graph,
    completed_nodes: &[String],
    node_outcomes: &HashMap<String, Outcome>,
) -> String {
    let goal = graph.goal();
    let run_id = context.get_string("run_id", "unknown");

    match fidelity {
        "truncate" => {
            format!("Goal: {goal}\nRun ID: {run_id}\n")
        }
        "compact" => {
            build_compact_preamble(goal, &run_id, completed_nodes, node_outcomes, context)
        }
        "summary:low" => {
            build_summary_preamble(goal, &run_id, completed_nodes, node_outcomes, context, SummaryDetail::Low)
        }
        "summary:medium" => {
            build_summary_preamble(goal, &run_id, completed_nodes, node_outcomes, context, SummaryDetail::Medium)
        }
        "summary:high" => {
            build_summary_preamble(goal, &run_id, completed_nodes, node_outcomes, context, SummaryDetail::High)
        }
        _ => {
            // Unknown fidelity mode: fall back to compact
            build_compact_preamble(goal, &run_id, completed_nodes, node_outcomes, context)
        }
    }
}

fn build_compact_preamble(
    goal: &str,
    run_id: &str,
    completed_nodes: &[String],
    node_outcomes: &HashMap<String, Outcome>,
    context: &Context,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Goal: {goal}"));
    parts.push(format!("Run ID: {run_id}"));

    if !completed_nodes.is_empty() {
        parts.push(String::from("\nCompleted stages:"));
        for node_id in completed_nodes {
            if let Some(outcome) = node_outcomes.get(node_id) {
                let status = outcome.status.to_string();
                let notes_suffix = outcome
                    .notes
                    .as_deref()
                    .map(|n| format!(" - {n}"))
                    .unwrap_or_default();
                parts.push(format!("- {node_id}: {status}{notes_suffix}"));
            } else {
                parts.push(format!("- {node_id}: completed"));
            }
        }
    }

    append_context_values(&mut parts, context);

    parts.push(String::new());
    parts.join("\n")
}

enum SummaryDetail {
    Low,
    Medium,
    High,
}

fn build_summary_preamble(
    goal: &str,
    run_id: &str,
    completed_nodes: &[String],
    node_outcomes: &HashMap<String, Outcome>,
    context: &Context,
    detail: SummaryDetail,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Goal: {goal}"));
    parts.push(format!("Run ID: {run_id}"));

    let stage_count = completed_nodes.len();
    parts.push(format!("Completed {stage_count} stage(s) so far."));

    // Determine how many recent stages to include based on detail level
    let recent_count = match detail {
        SummaryDetail::Low => 2,
        SummaryDetail::Medium => 5,
        SummaryDetail::High => stage_count,
    };

    let stages_to_show: Vec<&String> = if stage_count > recent_count {
        let skipped = stage_count - recent_count;
        parts.push(format!("\n({skipped} earlier stage(s) omitted)"));
        completed_nodes.iter().skip(skipped).collect()
    } else {
        completed_nodes.iter().collect()
    };

    if !stages_to_show.is_empty() {
        parts.push(String::from("\nRecent stages:"));
        for node_id in &stages_to_show {
            if let Some(outcome) = node_outcomes.get(*node_id) {
                let status = outcome.status.to_string();
                let mut line = format!("- {node_id}: {status}");
                if let Some(notes) = outcome.notes.as_deref() {
                    line.push_str(&format!(" ({notes})"));
                }
                if let Some(reason) = outcome.failure_reason.as_deref() {
                    line.push_str(&format!(" [reason: {reason}]"));
                }

                parts.push(line);

                // For medium and high detail, include context updates from the outcome
                match detail {
                    SummaryDetail::Medium | SummaryDetail::High => {
                        if !outcome.context_updates.is_empty() {
                            let mut update_keys: Vec<&String> = outcome.context_updates.keys().collect();
                            update_keys.sort();
                            for key in update_keys {
                                if let Some(val) = outcome.context_updates.get(key) {
                                    let val_str = if let Some(s) = val.as_str() {
                                        s.to_string()
                                    } else {
                                        val.to_string()
                                    };
                                    parts.push(format!("  - set {key} = {val_str}"));
                                }
                            }
                        }
                    }
                    SummaryDetail::Low => {}
                }
            } else {
                parts.push(format!("- {node_id}: completed"));
            }
        }
    }

    // Include context values for medium and high
    match detail {
        SummaryDetail::Medium | SummaryDetail::High => {
            append_context_values(&mut parts, context);
        }
        SummaryDetail::Low => {}
    }

    parts.push(String::new());
    parts.join("\n")
}

fn append_context_values(parts: &mut Vec<String>, context: &Context) {
    let snapshot = context.snapshot();
    let mut context_keys: Vec<&String> = snapshot
        .keys()
        .filter(|k| {
            !k.starts_with("internal.")
                && !k.starts_with("current")
                && *k != "run_id"
                && *k != "outcome"
        })
        .collect();
    if !context_keys.is_empty() {
        context_keys.sort();
        parts.push(String::from("\nContext values:"));
        for key in context_keys {
            if let Some(val) = snapshot.get(key) {
                let val_str = if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    val.to_string()
                };
                parts.push(format!("- {key}: {val_str}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::AttrValue;

    // --- truncate mode ---

    #[test]
    fn build_preamble_truncate_includes_goal_and_run_id() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Fix the login bug".to_string()),
        );
        let context = Context::new();
        context.set("run_id", serde_json::json!("abc-123"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "truncate",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("Fix the login bug"), "should contain the goal");
        assert!(preamble.contains("Run ID:"), "should contain run ID label");
        assert!(preamble.contains("abc-123"), "should contain the run ID value");
    }

    #[test]
    fn build_preamble_truncate_excludes_completed_stages() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Deploy app".to_string()),
        );
        let context = Context::new();
        let completed_nodes = vec!["plan".to_string(), "code".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("plan".to_string(), Outcome::success());
        node_outcomes.insert("code".to_string(), Outcome::success());

        let preamble = build_preamble(
            "truncate",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(!preamble.contains("plan"), "truncate should not list completed stages");
        assert!(!preamble.contains("code"), "truncate should not list completed stages");
    }

    // --- compact mode ---

    #[test]
    fn build_preamble_compact_lists_completed_stages() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Deploy app".to_string()),
        );
        let context = Context::new();
        context.set("run_id", serde_json::json!("run-456"));
        let completed_nodes = vec!["plan".to_string(), "code".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("plan".to_string(), Outcome::success());
        node_outcomes.insert("code".to_string(), Outcome::fail("compilation error"));

        let preamble = build_preamble(
            "compact",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("Deploy app"), "should contain the goal");
        assert!(preamble.contains("plan"), "should list completed stage 'plan'");
        assert!(preamble.contains("success"), "should show plan's success status");
        assert!(preamble.contains("code"), "should list completed stage 'code'");
        assert!(preamble.contains("fail"), "should show code's fail status");
    }

    #[test]
    fn build_preamble_compact_includes_context_values() {
        let graph = Graph::new("test");
        let context = Context::new();
        context.set("graph.goal", serde_json::json!("Build it"));
        context.set("user.name", serde_json::json!("alice"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "compact",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("graph.goal"), "should include graph.goal context key");
        assert!(preamble.contains("user.name"), "should include user.name context key");
        assert!(preamble.contains("alice"), "should include context value");
    }

    #[test]
    fn build_preamble_compact_excludes_internal_keys() {
        let graph = Graph::new("test");
        let context = Context::new();
        context.set("internal.fidelity", serde_json::json!("compact"));
        context.set("internal.retry_count.plan", serde_json::json!(1));
        context.set("current_node", serde_json::json!("work"));
        context.set("user.name", serde_json::json!("bob"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "compact",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(!preamble.contains("internal.fidelity"), "should exclude internal keys");
        assert!(!preamble.contains("internal.retry_count"), "should exclude internal keys");
        assert!(!preamble.contains("current_node"), "should exclude current keys");
        assert!(preamble.contains("user.name"), "should include non-internal keys");
    }

    #[test]
    fn build_preamble_compact_shows_notes_on_stages() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec!["work".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        let mut outcome = Outcome::success();
        outcome.notes = Some("auto-status: completed".to_string());
        node_outcomes.insert("work".to_string(), outcome);

        let preamble = build_preamble(
            "compact",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("auto-status: completed"), "should include outcome notes");
    }

    // --- summary:low mode ---

    #[test]
    fn build_preamble_summary_low_includes_stage_count() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Run tests".to_string()),
        );
        let context = Context::new();
        let completed_nodes = vec!["plan".to_string(), "code".to_string(), "test".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("plan".to_string(), Outcome::success());
        node_outcomes.insert("code".to_string(), Outcome::success());
        node_outcomes.insert("test".to_string(), Outcome::fail("test failure"));

        let preamble = build_preamble(
            "summary:low",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("Run tests"), "should contain the goal");
        assert!(preamble.contains("3 stage(s)"), "should mention total stage count");
    }

    #[test]
    fn build_preamble_summary_low_shows_only_recent_stages() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec![
            "step1".to_string(),
            "step2".to_string(),
            "step3".to_string(),
            "step4".to_string(),
        ];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("step1".to_string(), Outcome::success());
        node_outcomes.insert("step2".to_string(), Outcome::success());
        node_outcomes.insert("step3".to_string(), Outcome::success());
        node_outcomes.insert("step4".to_string(), Outcome::fail("error"));

        let preamble = build_preamble(
            "summary:low",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        // summary:low shows only 2 recent stages
        assert!(!preamble.contains("step1"), "should omit older stages");
        assert!(!preamble.contains("step2"), "should omit older stages");
        assert!(preamble.contains("step3"), "should show recent stage");
        assert!(preamble.contains("step4"), "should show most recent stage");
        assert!(preamble.contains("omitted"), "should indicate omitted stages");
    }

    #[test]
    fn build_preamble_summary_low_excludes_context_values() {
        let graph = Graph::new("test");
        let context = Context::new();
        context.set("user.name", serde_json::json!("alice"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "summary:low",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(!preamble.contains("user.name"), "summary:low should not include context values");
    }

    // --- summary:medium mode ---

    #[test]
    fn build_preamble_summary_medium_shows_more_stages_than_low() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec![
            "s1".to_string(),
            "s2".to_string(),
            "s3".to_string(),
            "s4".to_string(),
            "s5".to_string(),
            "s6".to_string(),
            "s7".to_string(),
        ];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("s1".to_string(), Outcome::success());
        node_outcomes.insert("s2".to_string(), Outcome::success());
        node_outcomes.insert("s3".to_string(), Outcome::success());
        node_outcomes.insert("s4".to_string(), Outcome::success());
        node_outcomes.insert("s5".to_string(), Outcome::success());
        node_outcomes.insert("s6".to_string(), Outcome::success());
        node_outcomes.insert("s7".to_string(), Outcome::success());

        let preamble = build_preamble(
            "summary:medium",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        // summary:medium shows 5 recent stages
        assert!(!preamble.contains("- s1:"), "should omit oldest stages");
        assert!(!preamble.contains("- s2:"), "should omit oldest stages");
        assert!(preamble.contains("s3"), "should show recent stage s3");
        assert!(preamble.contains("s7"), "should show most recent stage s7");
        assert!(preamble.contains("omitted"), "should indicate omitted stages");
    }

    #[test]
    fn build_preamble_summary_medium_includes_context_values() {
        let graph = Graph::new("test");
        let context = Context::new();
        context.set("user.name", serde_json::json!("alice"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "summary:medium",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("user.name"), "summary:medium should include context values");
        assert!(preamble.contains("alice"), "should include context value");
    }

    #[test]
    fn build_preamble_summary_medium_includes_context_updates() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec!["work".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        let mut outcome = Outcome::success();
        outcome.context_updates.insert("result.score".to_string(), serde_json::json!(95));
        node_outcomes.insert("work".to_string(), outcome);

        let preamble = build_preamble(
            "summary:medium",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("result.score"), "should include context updates from outcomes");
    }

    // --- summary:high mode ---

    #[test]
    fn build_preamble_summary_high_shows_all_stages() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec![
            "s1".to_string(),
            "s2".to_string(),
            "s3".to_string(),
            "s4".to_string(),
            "s5".to_string(),
            "s6".to_string(),
        ];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("s1".to_string(), Outcome::success());
        node_outcomes.insert("s2".to_string(), Outcome::success());
        node_outcomes.insert("s3".to_string(), Outcome::success());
        node_outcomes.insert("s4".to_string(), Outcome::success());
        node_outcomes.insert("s5".to_string(), Outcome::success());
        node_outcomes.insert("s6".to_string(), Outcome::success());

        let preamble = build_preamble(
            "summary:high",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        // summary:high shows ALL stages
        assert!(preamble.contains("s1"), "should show all stages including s1");
        assert!(preamble.contains("s6"), "should show all stages including s6");
        assert!(!preamble.contains("omitted"), "should not omit any stages");
    }

    #[test]
    fn build_preamble_summary_high_includes_failure_reasons() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes = vec!["work".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("work".to_string(), Outcome::fail("connection timeout"));

        let preamble = build_preamble(
            "summary:high",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("connection timeout"), "should include failure reason");
    }

    #[test]
    fn build_preamble_summary_high_includes_context_values() {
        let graph = Graph::new("test");
        let context = Context::new();
        context.set("graph.goal", serde_json::json!("Build"));
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "summary:high",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(preamble.contains("graph.goal"), "should include context values");
    }

    // --- unknown fidelity mode ---

    #[test]
    fn build_preamble_unknown_mode_falls_back_to_compact() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Test fallback".to_string()),
        );
        let context = Context::new();
        let completed_nodes = vec!["step1".to_string()];
        let mut node_outcomes: HashMap<String, Outcome> = HashMap::new();
        node_outcomes.insert("step1".to_string(), Outcome::success());

        let preamble = build_preamble(
            "unknown_mode",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        // Should behave like compact: include goal and stages
        assert!(preamble.contains("Test fallback"), "should contain the goal");
        assert!(preamble.contains("step1"), "should list completed stages like compact");
    }

    // --- empty state ---

    #[test]
    fn build_preamble_compact_with_no_stages() {
        let graph = Graph::new("test");
        let context = Context::new();
        let completed_nodes: Vec<String> = Vec::new();
        let node_outcomes: HashMap<String, Outcome> = HashMap::new();

        let preamble = build_preamble(
            "compact",
            &context,
            &graph,
            &completed_nodes,
            &node_outcomes,
        );

        assert!(!preamble.contains("Completed stages"), "should not show stages header when empty");
    }
}
