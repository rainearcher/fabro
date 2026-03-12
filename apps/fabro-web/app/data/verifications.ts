export type VerificationResult = "pass" | "fail" | "skip" | "na";

export type VerificationType = "ai" | "automated" | "analysis" | "ai-analysis";

export interface Criterion {
  name: string;
  description: string;
  type: VerificationType | null;
  status: VerificationResult;
}

export interface VerificationCategory {
  name: string;
  question: string;
  status: VerificationResult;
  criteria: Criterion[];
}

export const statusConfig = {
  pass: {
    label: "Pass",
    color: "text-mint",
    bg: "bg-mint/15",
    dot: "bg-mint",
    border: "border-l-mint/50",
  },
  fail: {
    label: "Fail",
    color: "text-coral",
    bg: "bg-coral/15",
    dot: "bg-coral",
    border: "border-l-coral/50",
  },
  skip: {
    label: "Skip",
    color: "text-fg-muted",
    bg: "bg-overlay",
    dot: "bg-fg-muted",
    border: "border-l-fg-muted/50",
  },
  na: {
    label: "N/A",
    color: "text-fg-muted",
    bg: "bg-overlay",
    dot: "bg-fg-muted",
    border: "border-l-fg-muted/50",
  },
} as const satisfies Record<
  VerificationResult,
  { label: string; color: string; bg: string; dot: string; border: string }
>;

export const typeConfig = {
  ai: { label: "AI", color: "text-teal-300", bg: "bg-teal-500/10" },
  automated: { label: "Automated", color: "text-mint", bg: "bg-mint/10" },
  analysis: { label: "Analysis", color: "text-amber", bg: "bg-amber/10" },
  "ai-analysis": { label: "AI + Analysis", color: "text-teal-300", bg: "bg-teal-500/10" },
} as const satisfies Record<
  VerificationType,
  { label: string; color: string; bg: string }
>;

export type VerificationMode = "active" | "evaluate" | "disabled";

export interface CriterionPerformance {
  f1: number | null;
  passAt1: number | null;
  mode: VerificationMode;
  evaluations: VerificationResult[];
}

export const modeConfig = {
  active: { label: "Active", color: "text-mint", bg: "bg-mint/10" },
  evaluate: { label: "Evaluate", color: "text-amber", bg: "bg-amber/10" },
  disabled: { label: "Disabled", color: "text-fg-muted", bg: "bg-overlay" },
} as const satisfies Record<
  VerificationMode,
  { label: string; color: string; bg: string }
>;

export function getCriteriaSummary(criteria: readonly Criterion[]) {
  return {
    passing: criteria.filter((c) => c.status === "pass").length,
    failing: criteria.filter((c) => c.status === "fail").length,
    na: criteria.filter((c) => c.status === "na").length,
    total: criteria.length,
  };
}

export function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
}

