import { useState } from "react";
import { Link, useParams } from "react-router";
import { ChevronRightIcon } from "@heroicons/react/20/solid";
import { CheckCircleIcon, ArrowPathIcon, PauseCircleIcon, XCircleIcon } from "@heroicons/react/24/solid";
import { DocumentTextIcon, MapIcon, CommandLineIcon, ChatBubbleLeftIcon, WrenchScrewdriverIcon } from "@heroicons/react/24/outline";
import { findRun } from "../data/runs";
import { workflowData } from "./workflow-detail";

export const handle = { wide: true };

type StageStatus = "completed" | "running" | "pending" | "failed";

interface Stage {
  id: string;
  name: string;
  status: StageStatus;
  duration: string;
}

const stages: Stage[] = [
  { id: "detect-drift", name: "Detect Drift", status: "completed", duration: "1m 12s" },
  { id: "propose-changes", name: "Propose Changes", status: "completed", duration: "2m 34s" },
  { id: "review-changes", name: "Review Changes", status: "completed", duration: "0m 45s" },
  { id: "apply-changes", name: "Apply Changes", status: "running", duration: "1m 58s" },
];

const statusConfig: Record<StageStatus, { icon: typeof CheckCircleIcon; color: string }> = {
  completed: { icon: CheckCircleIcon, color: "text-mint" },
  running: { icon: ArrowPathIcon, color: "text-teal-500" },
  pending: { icon: PauseCircleIcon, color: "text-navy-600" },
  failed: { icon: XCircleIcon, color: "text-coral" },
};

interface ToolUse {
  toolName: string;
  args: string;
  result: string;
}

type TurnType =
  | { kind: "system"; content: string }
  | { kind: "assistant"; content: string }
  | { kind: "tool"; tools: ToolUse[] };

// selectedStage is resolved from the URL param in RunStages below

const turns: TurnType[] = [
  {
    kind: "system",
    content: `You are a drift detection agent. Compare the production and staging environments and identify any configuration or code drift.\n\nSource: production\nTarget: staging\nThreshold: warn`,
  },
  {
    kind: "assistant",
    content: "I'll start by loading the environment configurations for both production and staging to compare them.",
  },
  {
    kind: "tool",
    tools: [
      {
        toolName: "read_file",
        args: `{ "path": "environments/production/config.toml" }`,
        result: `[redis]\nhost = "redis-prod.internal"\nport = 6379\nmax_connections = 200\ntls = true\n\n[iam]\nrole_arn = "arn:aws:iam::123456:role/prod-api"\nsession_duration = 3600`,
      },
      {
        toolName: "read_file",
        args: `{ "path": "environments/staging/config.toml" }`,
        result: `[redis]\nhost = "redis-staging.internal"\nport = 6379\nmax_connections = 100\ntls = false\n\n[iam]\nrole_arn = "arn:aws:iam::123456:role/staging-api"\nsession_duration = 1800`,
      },
      {
        toolName: "diff_configs",
        args: `{ "source": "environments/production/config.toml", "target": "environments/staging/config.toml" }`,
        result: `3 differences found:\n  redis.max_connections: 200 → 100\n  redis.tls: true → false\n  iam.session_duration: 3600 → 1800`,
      },
    ],
  },
  {
    kind: "assistant",
    content: "I've detected drift in 3 resources between production and staging:\n\n1. **redis.max_connections** — production has 200, staging has 100\n2. **redis.tls** — enabled in production, disabled in staging\n3. **iam.session_duration** — production uses 3600s, staging uses 1800s\n\nThe TLS mismatch is the most critical — staging should match production's TLS configuration for accurate testing. The connection pool and session duration differences may be intentional for cost reasons but should be verified.",
  },
];

function ToolRow({ tool }: { tool: ToolUse }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="border-b border-white/[0.04] last:border-b-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left transition-colors hover:bg-white/[0.02]"
      >
        <ChevronRightIcon className={`size-3 shrink-0 text-navy-600 transition-transform duration-150 ${open ? "rotate-90" : ""}`} />
        <WrenchScrewdriverIcon className="size-3.5 shrink-0 text-navy-600" />
        <span className="font-mono text-xs text-ice-300">{tool.toolName}</span>
        <span className="truncate font-mono text-xs text-navy-600">{tool.args}</span>
      </button>
      {open && (
        <div className="space-y-px bg-white/[0.01] px-2.5 pb-2 pt-1">
          <div className="rounded bg-white/[0.02] px-2.5 py-2">
            <div className="mb-1 text-[10px] font-medium uppercase tracking-wider text-navy-600">Args</div>
            <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-ice-300">{tool.args}</pre>
          </div>
          <div className="rounded bg-white/[0.02] px-2.5 py-2">
            <div className="mb-1 text-[10px] font-medium uppercase tracking-wider text-navy-600">Result</div>
            <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-ice-300">{tool.result}</pre>
          </div>
        </div>
      )}
    </div>
  );
}

function ToolBlock({ tools }: { tools: ToolUse[] }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.01] overflow-hidden">
      {tools.map((tool, i) => (
        <ToolRow key={i} tool={tool} />
      ))}
    </div>
  );
}

function SystemBlock({ content }: { content: string }) {
  return (
    <div className="rounded-lg border border-amber/10 bg-amber/5 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2">
        <CommandLineIcon className="size-4 shrink-0 text-amber" />
        <span className="text-xs font-medium text-ice-300">System Prompt</span>
      </div>
      <div className="border-t border-white/[0.04] px-3 py-2.5">
        <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-ice-300">{content}</pre>
      </div>
    </div>
  );
}

function AssistantBlock({ content }: { content: string }) {
  return (
    <div className="rounded-lg border border-teal-500/10 bg-teal-500/5 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2">
        <ChatBubbleLeftIcon className="size-4 shrink-0 text-teal-500" />
        <span className="text-xs font-medium text-ice-300">Assistant</span>
      </div>
      <div className="border-t border-white/[0.04] px-3 py-2.5">
        <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-ice-300">{content}</pre>
      </div>
    </div>
  );
}

export default function RunStages() {
  const { id, stageId } = useParams();
  const run = findRun(id ?? "");
  const workflow = run ? workflowData[run.workflow] : undefined;
  const selectedStage = stages.find((s) => s.id === stageId) ?? stages[0];
  const selectedConfig = statusConfig[selectedStage.status];
  const SelectedIcon = selectedConfig.icon;

  return (
    <div className="flex gap-6">
      <nav className="w-56 shrink-0 space-y-6">
        <div>
          <h3 className="px-2 text-xs font-medium uppercase tracking-wider text-navy-600">Stages</h3>
          <ul className="mt-2 space-y-0.5">
            {stages.map((stage) => {
              const config = statusConfig[stage.status];
              const Icon = config.icon;
              const isSelected = stage.id === selectedStage.id;
              return (
                <li key={stage.id}>
                  <Link
                    to={`/runs/${id}/stages/${stage.id}`}
                    className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors ${
                      isSelected
                        ? "bg-white/[0.06] text-white"
                        : "text-ice-300 hover:bg-white/[0.04] hover:text-white"
                    }`}
                  >
                    <Icon className={`size-4 shrink-0 ${config.color} ${stage.status === "running" ? "animate-spin" : ""}`} />
                    <span className="flex-1 truncate">{stage.name}</span>
                    <span className="font-mono text-xs tabular-nums text-navy-600">{stage.duration}</span>
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>

        {workflow && (
          <div>
            <h3 className="px-2 text-xs font-medium uppercase tracking-wider text-navy-600">Workflow</h3>
            <ul className="mt-2 space-y-0.5">
              <li>
                <Link
                  to={`/workflows/${run?.workflow}`}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ice-300 transition-colors hover:bg-white/[0.04] hover:text-white"
                >
                  <DocumentTextIcon className="size-4 shrink-0 text-navy-600" />
                  Run Configuration
                </Link>
              </li>
              <li>
                <Link
                  to={`/workflows/${run?.workflow}/diagram`}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ice-300 transition-colors hover:bg-white/[0.04] hover:text-white"
                >
                  <MapIcon className="size-4 shrink-0 text-navy-600" />
                  Workflow Graph
                </Link>
              </li>
            </ul>
          </div>
        )}
      </nav>

      <div className="min-w-0 flex-1 space-y-3">
        <div className="flex items-center gap-2">
          <SelectedIcon className={`size-5 ${selectedConfig.color}`} />
          <h3 className="text-sm font-medium text-white">{selectedStage.name}</h3>
          <span className="font-mono text-xs text-navy-600">{selectedStage.duration}</span>
        </div>

        {turns.map((turn, i) => {
          switch (turn.kind) {
            case "system":
              return <SystemBlock key={i} content={turn.content} />;
            case "assistant":
              return <AssistantBlock key={i} content={turn.content} />;
            case "tool":
              return <ToolBlock key={i} tools={turn.tools} />;
          }
        })}
      </div>
    </div>
  );
}
