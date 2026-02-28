export type CiStatus = "passing" | "failing" | "pending";

export interface RunItem {
  repo: string;
  title: string;
  number?: number;
  additions?: number;
  deletions?: number;
  ci?: CiStatus;
  elapsed?: string;
  elapsedWarning?: boolean;
  resources?: string;
  actionDisabled?: boolean;
  comments?: number;
}

export type ColumnStatus = "working" | "pending" | "review" | "merge";

export interface RunWithStatus extends RunItem {
  status: ColumnStatus;
  statusLabel: string;
}

export const columns: {
  id: ColumnStatus;
  name: string;
  accent: string;
  iconColor: string;
  iconType: "branch" | "pr";
  actions: string[];
  items: RunItem[];
}[] = [
  {
    id: "working",
    name: "Working",
    accent: "bg-teal-500",
    iconColor: "text-teal-500",
    iconType: "branch",
    actions: ["Watch", "Steer"],
    items: [
      {
        repo: "api-server",
        title: "Add rate limiting to auth endpoints",
        resources: "4 CPU / 8 GB",
        elapsed: "7m",
      },
      {
        repo: "web-dashboard",
        title: "Migrate to React Router v7",
        resources: "8 CPU / 16 GB",
        elapsed: "2h 15m",
      },
      {
        repo: "cli-tools",
        title: "Fix config parsing for nested values",
        resources: "2 CPU / 4 GB",
        elapsed: "45m",
      },
    ],
  },
  {
    id: "pending",
    name: "Pending",
    accent: "bg-amber",
    iconColor: "text-amber",
    iconType: "branch",
    actions: ["Answer Question"],
    items: [
      {
        repo: "api-server",
        title: "Update OpenAPI spec for v3",
        additions: 567,
        deletions: 234,
        elapsed: "1h 12m",
      },
      {
        repo: "shared-types",
        title: "Add pipeline event types",
        additions: 145,
        deletions: 23,
        elapsed: "28m",
      },
    ],
  },
  {
    id: "review",
    name: "Verify",
    accent: "bg-mint",
    iconColor: "text-mint",
    iconType: "pr",
    actions: ["Resolve"],
    items: [
      {
        repo: "web-dashboard",
        title: "Add dark mode toggle",
        number: 889,
        additions: 234,
        deletions: 67,
        ci: "failing",
        elapsed: "35m",
        comments: 4,
      },
      {
        repo: "infrastructure",
        title: "Terraform module for Redis cluster",
        number: 156,
        additions: 412,
        deletions: 0,
        ci: "pending",
        elapsed: "12m",
        actionDisabled: true,
        comments: 1,
      },
    ],
  },
  {
    id: "merge",
    name: "Merge",
    accent: "bg-teal-300",
    iconColor: "text-teal-300",
    iconType: "pr",
    actions: ["Merge"],
    items: [
      {
        repo: "api-server",
        title: "Implement webhook retry logic",
        number: 1249,
        additions: 189,
        deletions: 45,
        ci: "passing",
        elapsed: "3d",
        elapsedWarning: true,
        comments: 7,
      },
      {
        repo: "cli-tools",
        title: "Add --verbose flag to run command",
        number: 430,
        additions: 56,
        deletions: 12,
        ci: "passing",
        elapsed: "1h 5m",
        comments: 2,
      },
      {
        repo: "shared-types",
        title: "Export utility type helpers",
        number: 76,
        additions: 34,
        deletions: 8,
        ci: "passing",
        elapsed: "48m",
        comments: 0,
      },
    ],
  },
];

export function allRunsFlat(): RunWithStatus[] {
  return [...columns].reverse().flatMap((col) =>
    col.items.map((item) => ({
      ...item,
      status: col.id,
      statusLabel: col.name,
    })),
  );
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
