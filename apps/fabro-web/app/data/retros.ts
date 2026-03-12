import { formatDurationSecs } from "../lib/format";

export type SmoothnessRating = "effortless" | "smooth" | "bumpy" | "struggled" | "failed";

type LearningCategory = "repo" | "code" | "workflow" | "tool";

export interface Learning {
  category: LearningCategory;
  text: string;
}

type FrictionKind = "retry" | "timeout" | "wrong_approach" | "tool_failure" | "ambiguity";

export interface FrictionPoint {
  kind: FrictionKind;
  description: string;
  stage_id?: string;
}

type OpenItemKind = "tech_debt" | "follow_up" | "investigation" | "test_gap";

export interface OpenItem {
  kind: OpenItemKind;
  description: string;
}

export interface StageRetro {
  stage_id: string;
  stage_label: string;
  status: string;
  duration_ms: number;
  retries: number;
  cost?: number;
  notes?: string;
  failure_reason?: string;
  files_touched: string[];
}

export interface AggregateStats {
  total_duration_ms: number;
  total_cost?: number;
  total_retries: number;
  files_touched: string[];
  stages_completed: number;
  stages_failed: number;
}

export interface Retro {
  run_id: string;
  workflow_name: string;
  goal: string;
  timestamp: string;
  smoothness?: SmoothnessRating;
  stages: StageRetro[];
  stats: AggregateStats;
  intent?: string;
  outcome?: string;
  learnings?: Learning[];
  friction_points?: FrictionPoint[];
  open_items?: OpenItem[];
}

export const smoothnessConfig: Record<SmoothnessRating, { label: string; bg: string; text: string; dot: string }> = {
  effortless: { label: "Effortless", bg: "bg-emerald-500/15", text: "text-emerald-400", dot: "bg-emerald-400" },
  smooth: { label: "Smooth", bg: "bg-mint/15", text: "text-mint", dot: "bg-mint" },
  bumpy: { label: "Bumpy", bg: "bg-amber/15", text: "text-amber", dot: "bg-amber" },
  struggled: { label: "Struggled", bg: "bg-orange-500/15", text: "text-orange-400", dot: "bg-orange-400" },
  failed: { label: "Failed", bg: "bg-coral/15", text: "text-coral", dot: "bg-coral" },
};

export const learningCategoryConfig: Record<LearningCategory, { label: string; text: string }> = {
  repo: { label: "Repo", text: "text-teal-400" },
  code: { label: "Code", text: "text-sky-400" },
  workflow: { label: "Workflow", text: "text-violet-400" },
  tool: { label: "Tool", text: "text-amber" },
};

export const frictionKindConfig: Record<FrictionKind, { label: string; text: string }> = {
  retry: { label: "Retry", text: "text-amber" },
  timeout: { label: "Timeout", text: "text-coral" },
  wrong_approach: { label: "Wrong Approach", text: "text-orange-400" },
  tool_failure: { label: "Tool Failure", text: "text-coral" },
  ambiguity: { label: "Ambiguity", text: "text-violet-400" },
};

export const openItemKindConfig: Record<OpenItemKind, { label: string; text: string }> = {
  tech_debt: { label: "Tech Debt", text: "text-orange-400" },
  follow_up: { label: "Follow-up", text: "text-teal-400" },
  investigation: { label: "Investigation", text: "text-sky-400" },
  test_gap: { label: "Test Gap", text: "text-coral" },
};

function formatDurationMs(ms: number): string {
  return formatDurationSecs(Math.floor(ms / 1000));
}

export { formatDurationMs };
