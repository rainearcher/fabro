import { formatElapsedSecs, formatDurationSecs } from "../lib/format";
import type { RunListItem } from "@qltysh/fabro-api-client";

export type CiStatus = "passing" | "failing" | "pending";

export type CheckStatus = "success" | "failure" | "skipped" | "pending" | "queued";

export interface CheckRun {
  name: string;
  status: CheckStatus;
  duration?: string;
}

export interface RunItem {
  id: string;
  repo: string;
  title: string;
  workflow: string;
  number?: number;
  additions?: number;
  deletions?: number;
  checks?: CheckRun[];
  elapsed?: string;
  elapsedWarning?: boolean;
  resources?: string;
  actionDisabled?: boolean;
  comments?: number;
  question?: string;
  sandboxId?: string;
}

export type ColumnStatus = "working" | "pending" | "review" | "merge";

export const columnNames: Record<ColumnStatus, string> = {
  working: "Working",
  pending: "Pending",
  review: "Verify",
  merge: "Merge",
};

export interface RunWithStatus extends RunItem {
  status: ColumnStatus;
  statusLabel: string;
}

export function mapRunListItem(item: RunListItem): RunItem {
  return {
    id: item.id,
    repo: item.repository.name,
    title: item.title,
    workflow: item.workflow.slug,
    number: item.pull_request?.number,
    additions: item.pull_request?.additions,
    deletions: item.pull_request?.deletions,
    checks: item.pull_request?.checks?.map((c) => ({
      name: c.name,
      status: c.status,
      duration: c.duration_secs != null ? formatDurationSecs(c.duration_secs) : undefined,
    })),
    elapsed: item.timings?.elapsed_secs != null ? formatElapsedSecs(item.timings.elapsed_secs) : undefined,
    elapsedWarning: item.timings?.elapsed_warning,
    resources: item.sandbox?.resources ? `${item.sandbox.resources.cpu} CPU / ${item.sandbox.resources.memory} GB` : undefined,
    comments: item.pull_request?.comments,
    question: item.question?.text,
    sandboxId: item.sandbox?.id,
  };
}

export function deriveCiStatus(checks: CheckRun[]): CiStatus {
  if (checks.some((c) => c.status === "failure")) return "failing";
  if (checks.some((c) => c.status === "pending" || c.status === "queued")) return "pending";
  return "passing";
}

export const statusColors: Record<ColumnStatus, { dot: string; text: string }> = {
  working: { dot: "bg-teal-500", text: "text-teal-500" },
  pending: { dot: "bg-amber", text: "text-amber" },
  review: { dot: "bg-mint", text: "text-mint" },
  merge: { dot: "bg-teal-300", text: "text-teal-300" },
};

export const ciConfig: Record<CiStatus, { label: string; dot: string; text: string }> = {
  passing: { label: "Passing", dot: "bg-mint", text: "text-mint" },
  failing: { label: "Changes needed", dot: "bg-coral", text: "text-coral" },
  pending: { label: "Pending", dot: "bg-amber", text: "text-amber" },
};
