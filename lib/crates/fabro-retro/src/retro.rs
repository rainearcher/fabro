use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Flat summary of a completed stage, built by callers from their own
/// checkpoint/outcome types to decouple retro derivation from the workflow
/// engine internals.
#[derive(Debug, Clone)]
pub struct CompletedStage {
    pub node_id: String,
    pub status: String,
    pub succeeded: bool,
    pub failed: bool,
    pub retries: u32,
    pub cost: Option<f64>,
    pub notes: Option<String>,
    pub failure_reason: Option<String>,
    pub files_touched: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmoothnessRating {
    Effortless,
    Smooth,
    Bumpy,
    Struggled,
    Failed,
}

impl fmt::Display for SmoothnessRating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SmoothnessRating::Effortless => "effortless",
            SmoothnessRating::Smooth => "smooth",
            SmoothnessRating::Bumpy => "bumpy",
            SmoothnessRating::Struggled => "struggled",
            SmoothnessRating::Failed => "failed",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningCategory {
    Repo,
    Code,
    Workflow,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learning {
    pub category: LearningCategory,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrictionKind {
    Retry,
    Timeout,
    WrongApproach,
    ToolFailure,
    Ambiguity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrictionPoint {
    pub kind: FrictionKind,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenItemKind {
    TechDebt,
    FollowUp,
    Investigation,
    TestGap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenItem {
    pub kind: OpenItemKind,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRetro {
    pub stage_id: String,
    pub stage_label: String,
    pub status: String,
    pub duration_ms: u64,
    pub retries: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_touched: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateStats {
    pub total_duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
    pub total_retries: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_touched: Vec<String>,
    pub stages_completed: usize,
    pub stages_failed: usize,
}

/// Agent-generated qualitative narrative fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetroNarrative {
    pub smoothness: SmoothnessRating,
    pub intent: String,
    pub outcome: String,
    #[serde(default)]
    pub learnings: Vec<Learning>,
    #[serde(default)]
    pub friction_points: Vec<FrictionPoint>,
    #[serde(default)]
    pub open_items: Vec<OpenItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retro {
    pub run_id: String,
    pub workflow_name: String,
    pub goal: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothness: Option<SmoothnessRating>,
    pub stages: Vec<StageRetro>,
    pub stats: AggregateStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learnings: Option<Vec<Learning>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friction_points: Option<Vec<FrictionPoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_items: Option<Vec<OpenItem>>,
}

impl Retro {
    /// Merge agent-generated narrative into this retro.
    pub fn apply_narrative(&mut self, narrative: RetroNarrative) {
        self.smoothness = Some(narrative.smoothness);
        self.intent = Some(narrative.intent);
        self.outcome = Some(narrative.outcome);
        self.learnings = if narrative.learnings.is_empty() {
            None
        } else {
            Some(narrative.learnings)
        };
        self.friction_points = if narrative.friction_points.is_empty() {
            None
        } else {
            Some(narrative.friction_points)
        };
        self.open_items = if narrative.open_items.is_empty() {
            None
        } else {
            Some(narrative.open_items)
        };
    }

    /// Save the retro as JSON to `run_dir/retro.json`.
    pub fn save(&self, run_dir: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("retro serialize failed: {e}"))?;
        std::fs::write(run_dir.join("retro.json"), json)?;
        Ok(())
    }

    /// Load a retro from `run_dir/retro.json`.
    pub fn load(run_dir: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(run_dir.join("retro.json"))?;
        serde_json::from_str(&data).map_err(|e| anyhow::anyhow!("retro deserialize failed: {e}"))
    }
}

/// Extract stage durations from `progress.jsonl` by reading `StageCompleted` events.
pub fn extract_stage_durations(run_dir: &Path) -> HashMap<String, u64> {
    let mut durations = HashMap::new();
    let jsonl_path = run_dir.join("progress.jsonl");
    let Ok(data) = std::fs::read_to_string(&jsonl_path) else {
        return durations;
    };
    for line in data.lines() {
        let Ok(envelope) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if envelope.get("event").and_then(|v| v.as_str()) != Some("StageCompleted") {
            continue;
        }
        let Some(name) = envelope.get("node_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(duration_ms) = envelope.get("duration_ms").and_then(|v| v.as_u64()) else {
            continue;
        };
        durations.insert(name.to_string(), duration_ms);
    }
    durations
}

/// Build a `Retro` from completed stage data and run metadata. All qualitative
/// fields (`smoothness`, `intent`, `outcome`, etc.) are left as `None` for
/// the retro agent to fill in.
pub fn derive_retro(
    run_id: &str,
    workflow_name: &str,
    goal: &str,
    completed_stages: Vec<CompletedStage>,
    duration_ms: u64,
    stage_durations: &HashMap<String, u64>,
) -> Retro {
    let mut stages = Vec::new();
    let mut all_files: Vec<String> = Vec::new();
    let mut total_cost: Option<f64> = None;
    let mut total_retries: u32 = 0;
    let mut stages_completed: usize = 0;
    let mut stages_failed: usize = 0;

    for cs in completed_stages {
        total_retries += cs.retries;

        if cs.succeeded {
            stages_completed += 1;
        }
        if cs.failed {
            stages_failed += 1;
        }

        if let Some(c) = cs.cost {
            *total_cost.get_or_insert(0.0) += c;
        }

        let dur = stage_durations.get(&cs.node_id).copied().unwrap_or(0);

        stages.push(StageRetro {
            stage_label: cs.node_id.clone(),
            duration_ms: dur,
            retries: cs.retries,
            cost: cs.cost,
            stage_id: cs.node_id,
            status: cs.status,
            notes: cs.notes,
            failure_reason: cs.failure_reason,
            files_touched: cs.files_touched,
        });

        all_files.extend(stages.last().unwrap().files_touched.iter().cloned());
    }

    all_files.sort();
    all_files.dedup();

    let stats = AggregateStats {
        total_duration_ms: duration_ms,
        total_cost,
        total_retries,
        files_touched: all_files,
        stages_completed,
        stages_failed,
    };

    Retro {
        run_id: run_id.to_string(),
        workflow_name: workflow_name.to_string(),
        goal: goal.to_string(),
        timestamp: Utc::now(),
        smoothness: None,
        stages,
        stats,
        intent: None,
        outcome: None,
        learnings: None,
        friction_points: None,
        open_items: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_completed_stages() -> Vec<CompletedStage> {
        vec![
            CompletedStage {
                node_id: "plan".to_string(),
                status: "success".to_string(),
                succeeded: true,
                failed: false,
                retries: 0,
                cost: Some(0.05),
                notes: Some("Planned the approach".to_string()),
                failure_reason: None,
                files_touched: vec!["src/main.rs".to_string()],
            },
            CompletedStage {
                node_id: "code".to_string(),
                status: "success".to_string(),
                succeeded: true,
                failed: false,
                retries: 1,
                cost: Some(0.10),
                notes: None,
                failure_reason: None,
                files_touched: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            },
        ]
    }

    #[test]
    fn derive_retro_builds_stages() {
        let stages = make_completed_stages();
        let durations: HashMap<String, u64> =
            [("plan".to_string(), 5000), ("code".to_string(), 15000)]
                .into_iter()
                .collect();

        let retro = derive_retro(
            "run-1",
            "my_pipeline",
            "Fix the bug",
            stages.clone(),
            20000,
            &durations,
        );

        assert_eq!(retro.run_id, "run-1");
        assert_eq!(retro.workflow_name, "my_pipeline");
        assert_eq!(retro.goal, "Fix the bug");
        assert_eq!(retro.stages.len(), 2);
        assert_eq!(retro.stages[0].stage_id, "plan");
        assert_eq!(retro.stages[0].duration_ms, 5000);
        assert_eq!(retro.stages[0].retries, 0);
        assert_eq!(retro.stages[1].stage_id, "code");
        assert_eq!(retro.stages[1].duration_ms, 15000);
        assert_eq!(retro.stages[1].retries, 1);
        assert_eq!(retro.stats.total_duration_ms, 20000);
        assert_eq!(retro.stats.total_retries, 1);
        assert_eq!(retro.stats.stages_completed, 2);
        assert_eq!(retro.stats.stages_failed, 0);
        assert!((retro.stats.total_cost.unwrap() - 0.15).abs() < f64::EPSILON);
        assert_eq!(retro.stats.files_touched, vec!["src/lib.rs", "src/main.rs"]);
        assert!(retro.smoothness.is_none());
        assert!(retro.intent.is_none());
    }

    #[test]
    fn derive_retro_handles_failed_stage() {
        let stages = vec![CompletedStage {
            node_id: "start".to_string(),
            status: "fail".to_string(),
            succeeded: false,
            failed: true,
            retries: 0,
            cost: None,
            notes: None,
            failure_reason: Some("boom".to_string()),
            files_touched: vec![],
        }];

        let retro = derive_retro(
            "run-2",
            "pipe",
            "goal",
            stages.clone(),
            5000,
            &HashMap::new(),
        );

        assert_eq!(retro.stats.stages_failed, 1);
        assert_eq!(retro.stats.stages_completed, 0);
    }

    #[test]
    fn apply_narrative_merges_fields() {
        let stages = make_completed_stages();
        let mut retro = derive_retro("r1", "p", "g", stages.clone(), 1000, &HashMap::new());

        let narrative = RetroNarrative {
            smoothness: SmoothnessRating::Smooth,
            intent: "Fix authentication bug".to_string(),
            outcome: "Successfully fixed the login flow".to_string(),
            learnings: vec![Learning {
                category: LearningCategory::Code,
                text: "Token refresh logic was in the wrong module".to_string(),
            }],
            friction_points: vec![],
            open_items: vec![OpenItem {
                kind: OpenItemKind::TestGap,
                description: "No integration test for token refresh".to_string(),
            }],
        };

        retro.apply_narrative(narrative);

        assert_eq!(retro.smoothness, Some(SmoothnessRating::Smooth));
        assert_eq!(retro.intent.as_deref(), Some("Fix authentication bug"));
        assert_eq!(
            retro.outcome.as_deref(),
            Some("Successfully fixed the login flow")
        );
        assert_eq!(retro.learnings.as_ref().unwrap().len(), 1);
        assert!(retro.friction_points.is_none()); // empty vec -> None
        assert_eq!(retro.open_items.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let stages = make_completed_stages();
        let mut retro = derive_retro("r1", "pipe", "goal", stages.clone(), 1000, &HashMap::new());
        retro.smoothness = Some(SmoothnessRating::Bumpy);
        retro.intent = Some("Test intent".to_string());

        retro.save(dir.path()).unwrap();
        let loaded = Retro::load(dir.path()).unwrap();

        assert_eq!(loaded.run_id, "r1");
        assert_eq!(loaded.smoothness, Some(SmoothnessRating::Bumpy));
        assert_eq!(loaded.intent.as_deref(), Some("Test intent"));
        assert_eq!(loaded.stages.len(), 2);
    }

    #[test]
    fn load_nonexistent_retro() {
        let result = Retro::load(Path::new("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn smoothness_rating_serde_roundtrip() {
        let json = serde_json::to_string(&SmoothnessRating::Effortless).unwrap();
        assert_eq!(json, "\"effortless\"");
        let parsed: SmoothnessRating = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, SmoothnessRating::Effortless);
    }

    #[test]
    fn retro_narrative_serde() {
        let narrative = RetroNarrative {
            smoothness: SmoothnessRating::Failed,
            intent: "Deploy feature".to_string(),
            outcome: "Build failed".to_string(),
            learnings: vec![],
            friction_points: vec![FrictionPoint {
                kind: FrictionKind::ToolFailure,
                description: "Compiler error".to_string(),
                stage_id: Some("build".to_string()),
            }],
            open_items: vec![],
        };
        let json = serde_json::to_string(&narrative).unwrap();
        let parsed: RetroNarrative = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.smoothness, SmoothnessRating::Failed);
        assert_eq!(parsed.friction_points.len(), 1);
        assert_eq!(parsed.friction_points[0].kind, FrictionKind::ToolFailure);
    }

    #[test]
    fn extract_stage_durations_from_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("progress.jsonl");

        let event1 = serde_json::json!({
            "ts": "2025-01-01T00:00:00.000Z",
            "run_id": "r1",
            "event": "StageCompleted",
            "node_id": "plan",
            "node_label": "Plan",
            "stage_index": 0,
            "duration_ms": 5000,
            "status": "success",
            "preferred_label": null,
            "suggested_next_ids": [],
            "usage": null,
            "failure_reason": null,
            "notes": null,
            "files_touched": [],
            "attempt": 1,
            "max_attempts": 1,
            "failure_class": null
        });
        let event2 = serde_json::json!({
            "ts": "2025-01-01T00:00:05.000Z",
            "run_id": "r1",
            "event": "StageCompleted",
            "node_id": "code",
            "node_label": "Code",
            "stage_index": 1,
            "duration_ms": 15000,
            "status": "success",
            "preferred_label": null,
            "suggested_next_ids": [],
            "usage": null,
            "failure_reason": null,
            "notes": null,
            "files_touched": [],
            "attempt": 1,
            "max_attempts": 1,
            "failure_class": null
        });
        let content = format!(
            "{}\n{}\n",
            serde_json::to_string(&event1).unwrap(),
            serde_json::to_string(&event2).unwrap()
        );
        std::fs::write(&jsonl, content).unwrap();

        let durations = extract_stage_durations(dir.path());
        assert_eq!(durations.get("plan"), Some(&5000));
        assert_eq!(durations.get("code"), Some(&15000));
    }

    #[test]
    fn extract_stage_durations_missing_file() {
        let durations = extract_stage_durations(Path::new("/nonexistent"));
        assert!(durations.is_empty());
    }
}
