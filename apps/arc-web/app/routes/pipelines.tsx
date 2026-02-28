import type { Route } from "./+types/pipelines";

export function meta({}: Route.MetaArgs) {
  return [{ title: "Pipeline Runs — Arc" }];
}


function GitBranchIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="currentColor"
      className={className}
      aria-hidden="true"
    >
      <path d="M9.5 3.25a2.25 2.25 0 1 1 3 2.122V6A2.5 2.5 0 0 1 10 8.5H6a1 1 0 0 0-1 1v1.128a2.251 2.251 0 1 1-1.5 0V5.372a2.25 2.25 0 1 1 1.5 0v1.836A2.5 2.5 0 0 1 6 7h4a1 1 0 0 0 1-1v-.628A2.25 2.25 0 0 1 9.5 3.25Zm-6 0a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Zm8.25-.75a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5ZM4.25 12a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Z" />
    </svg>
  );
}

function GitPullRequestIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="currentColor"
      className={className}
      aria-hidden="true"
    >
      <path d="M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z" />
    </svg>
  );
}

type CiStatus = "passing" | "failing" | "pending";

const ciConfig: Record<CiStatus, { label: string; dot: string; text: string }> =
  {
    passing: { label: "Passing", dot: "bg-mint", text: "text-mint" },
    failing: {
      label: "Changes needed",
      dot: "bg-coral",
      text: "text-coral",
    },
    pending: { label: "Pending", dot: "bg-amber", text: "text-amber" },
  };

function CiBadge({ status }: { status: CiStatus }) {
  const config = ciConfig[status];
  return (
    <span className={`inline-flex items-center gap-1.5 font-mono text-xs ${config.text}`}>
      <span className={`size-1.5 rounded-full ${config.dot}`} />
      {config.label}
    </span>
  );
}

interface PullRequest {
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

interface Column {
  id: string;
  name: string;
  accent: string;
  iconColor: string;
  icon: React.ComponentType<{ className?: string }>;
  actions?: string[];
  items: PullRequest[];
}

const columns: Column[] = [
  {
    id: "working",
    name: "Working",
    accent: "bg-teal-500",
    iconColor: "text-teal-500",
    icon: GitBranchIcon,
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
    icon: GitBranchIcon,
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
    icon: GitPullRequestIcon,
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
    icon: GitPullRequestIcon,
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

const totalCards = columns.reduce((sum, col) => sum + col.items.length, 0);
const totalPrs = columns.reduce(
  (sum, col) => sum + col.items.filter((item) => item.number != null).length,
  0,
);

export const handle = {
  headerExtra: (
    <div className="flex items-center gap-4 font-mono text-xs text-ice-300">
      <span>
        <span className="text-white">{totalCards}</span> runs
      </span>
      <span>
        <span className="text-white">{totalPrs}</span> PRs
      </span>
    </div>
  ),
};

function PrCard({
  pr,
  icon: Icon,
  iconColor,
  actions,
}: {
  pr: PullRequest;
  icon: React.ComponentType<{ className?: string }>;
  iconColor: string;
  actions?: string[];
}) {
  return (
    <div className="group rounded-lg border border-white/[0.06] bg-navy-800/80 p-4 transition-all duration-200 hover:border-white/[0.12] hover:bg-navy-800 hover:shadow-lg hover:shadow-black/20">
      <div className="mb-2 flex items-center gap-1.5">
        <Icon className={`size-3.5 shrink-0 ${iconColor}`} />
        <span className="font-mono text-xs font-medium text-teal-500">
          {pr.repo}
        </span>
        {pr.number != null && (
          <span className="font-mono text-xs text-navy-600">
            #{pr.number}
          </span>
        )}
      </div>

      <p className="text-sm leading-snug text-ice-100">{pr.title}</p>

      {(pr.additions != null || pr.resources != null || pr.ci != null || pr.elapsed != null) && (
        <div className="mt-3 flex items-center gap-3 font-mono text-xs">
          {pr.resources != null && (
            <span className="text-ice-300">{pr.resources}</span>
          )}
          {pr.additions != null && pr.deletions != null && (
            <>
              <span className="text-mint">
                +{pr.additions.toLocaleString()}
              </span>
              <span className="text-coral">
                -{pr.deletions.toLocaleString()}
              </span>
            </>
          )}
          {pr.comments != null && (
            <span className="inline-flex items-center gap-1 text-navy-600">
              <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                <path d="M1 2.75C1 1.784 1.784 1 2.75 1h10.5c.966 0 1.75.784 1.75 1.75v7.5A1.75 1.75 0 0 1 13.25 12H9.06l-2.573 2.573A1.458 1.458 0 0 1 4 13.543V12H2.75A1.75 1.75 0 0 1 1 10.25Zm1.75-.25a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h4.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25Z" />
              </svg>
              {pr.comments}
            </span>
          )}
          {pr.elapsed != null && (
            <span className={`ml-auto font-mono ${pr.elapsedWarning ? "text-amber" : "text-navy-600"}`}>{pr.elapsed}</span>
          )}
        </div>
      )}

      {(actions != null && actions.length > 0 || pr.ci != null) && (
        <div className="mt-3 flex items-center gap-1.5">
          {actions?.map((label) => (
            <button
              key={label}
              type="button"
              disabled={pr.actionDisabled}
              className={`inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors disabled:cursor-not-allowed disabled:text-navy-600 disabled:border-white/[0.04] ${
                label === "Merge"
                  ? "border-mint/20 text-mint hover:border-mint/50 hover:text-white"
                  : label === "Answer Question"
                    ? "border-amber/20 text-amber hover:border-amber/50 hover:text-white"
                    : label === "Resolve"
                      ? "border-teal-500/20 text-teal-500 hover:border-teal-500/50 hover:text-white"
                      : "border-white/[0.08] text-ice-300 hover:border-teal-500/40 hover:text-white"
              }`}
            >
              {label === "Watch" && (
                <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                  <path d="M8 2c1.981 0 3.671.992 4.933 2.078 1.27 1.091 2.187 2.345 2.637 3.023a1.62 1.62 0 0 1 0 1.798c-.45.678-1.367 1.932-2.637 3.023C11.67 13.008 9.981 14 8 14c-1.981 0-3.671-.992-4.933-2.078C1.797 10.831.88 9.577.43 8.899a1.62 1.62 0 0 1 0-1.798c.45-.678 1.367-1.932 2.637-3.023C4.33 2.992 6.019 2 8 2ZM1.679 7.932a.12.12 0 0 0 0 .136c.411.622 1.241 1.75 2.366 2.717C5.176 11.758 6.527 12.5 8 12.5c1.473 0 2.825-.742 3.955-1.715 1.124-.967 1.954-2.096 2.366-2.717a.12.12 0 0 0 0-.136c-.412-.621-1.242-1.75-2.366-2.717C10.824 4.242 9.473 3.5 8 3.5c-1.473 0-2.824.742-3.955 1.715-1.124.967-1.954 2.096-2.366 2.717ZM8 10a2 2 0 1 1-.001-3.999A2 2 0 0 1 8 10Z" />
                </svg>
              )}
              {label === "Steer" && (
                <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                  <path d="M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Zm7.25-4.5a.75.75 0 0 0-1.5 0v.582a2.75 2.75 0 0 0-2.168 2.168H4.5a.75.75 0 0 0 0 1.5h.582a2.75 2.75 0 0 0 2.168 2.168v.582a.75.75 0 0 0 1.5 0v-.582a2.75 2.75 0 0 0 2.168-2.168h.582a.75.75 0 0 0 0-1.5h-.582A2.75 2.75 0 0 0 8.75 4.082ZM8 6.75a1.25 1.25 0 1 0 0 2.5 1.25 1.25 0 0 0 0-2.5Z" />
                </svg>
              )}
              {label === "Answer Question" && (
                <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                  <path d="M1 2.75C1 1.784 1.784 1 2.75 1h10.5c.966 0 1.75.784 1.75 1.75v7.5A1.75 1.75 0 0 1 13.25 12H9.06l-2.573 2.573A1.458 1.458 0 0 1 4 13.543V12H2.75A1.75 1.75 0 0 1 1 10.25Zm1.75-.25a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h4.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25Z" />
                </svg>
              )}
              {label === "Resolve" && (
                <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                  <path d="M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z" />
                </svg>
              )}
              {label === "Merge" && (
                <svg viewBox="0 0 16 16" fill="currentColor" className="size-3" aria-hidden="true">
                  <path d="M5.45 5.154A4.25 4.25 0 0 0 9.25 7.5h1.378a2.251 2.251 0 1 1 0 1.5H9.25A5.734 5.734 0 0 1 5 7.123v3.505a2.25 2.25 0 1 1-1.5 0V5.372a2.25 2.25 0 1 1 1.95-.218ZM4.25 13.5a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Zm8-8a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5ZM4.25 4a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Z" />
                </svg>
              )}
              {label}
            </button>
          ))}
          {pr.ci != null && <span className="ml-auto flex items-center"><CiBadge status={pr.ci} /></span>}
        </div>
      )}
    </div>
  );
}

function BoardColumn({ column }: { column: Column }) {
  return (
    <div className="flex min-w-[280px] flex-1 flex-col">
      <div className="mb-4 flex items-center gap-3">
        <div className={`h-2.5 w-2.5 rounded-full ${column.accent}`} />
        <h3 className="text-sm font-semibold tracking-wide text-ice-100">
          {column.name}
        </h3>
        <span className="rounded-full bg-white/[0.06] px-2 py-0.5 font-mono text-xs text-navy-600">
          {column.items.length}
        </span>
      </div>

      <div className="flex flex-1 flex-col gap-3">
        {column.items.map((pr) => (
          <PrCard
            key={`${pr.repo}-${pr.number ?? pr.title}`}
            pr={pr}
            icon={column.icon}
            iconColor={column.iconColor}
            actions={column.actions}
          />
        ))}
      </div>
    </div>
  );
}

export default function Pipelines() {
  return (
    <div className="flex gap-5 overflow-x-auto pb-4">
      {columns.map((col) => (
        <BoardColumn key={col.id} column={col} />
      ))}
    </div>
  );
}
