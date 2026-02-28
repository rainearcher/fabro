import type { Route } from "./+types/workflows";

export function meta({}: Route.MetaArgs) {
  return [{ title: "Workflows — Arc" }];
}

interface Workflow {
  name: string;
  filename: string;
  lastRun: string;
}

const workflows: Workflow[] = [
  { name: "Fix Build", filename: "fix_build.dot", lastRun: "2 hours ago" },
  { name: "Implement Feature", filename: "implement.dot", lastRun: "4 days ago" },
  { name: "Sync Drift", filename: "sync_drift.dot", lastRun: "1 day ago" },
  { name: "Expand Product", filename: "expand.dot", lastRun: "2 weeks ago" },
];

function PlayIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
      <path fillRule="evenodd" d="M4.5 5.653c0-1.427 1.529-2.33 2.779-1.643l11.54 6.347c1.295.712 1.295 2.573 0 3.286L7.28 19.99c-1.25.687-2.779-.217-2.779-1.643V5.653Z" clipRule="evenodd" />
    </svg>
  );
}

function EllipsisIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
      <path fillRule="evenodd" d="M10.5 12a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0Zm6 0a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0Zm-12 0a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0Z" clipRule="evenodd" />
    </svg>
  );
}

function WorkflowCard({ workflow }: { workflow: Workflow }) {
  return (
    <div className="group flex items-center gap-4 rounded-lg border border-white/[0.06] bg-navy-800/80 p-4 transition-all duration-200 hover:border-white/[0.12] hover:bg-navy-800 hover:shadow-lg hover:shadow-black/20">
      <button
        type="button"
        title="Run workflow"
        className="flex size-9 shrink-0 items-center justify-center rounded-md border border-mint/20 text-mint transition-colors hover:border-mint/50 hover:bg-mint/10 hover:text-white"
      >
        <PlayIcon className="size-4" />
      </button>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-ice-100">{workflow.name}</span>
          <span className="font-mono text-xs text-navy-600">{workflow.filename}</span>
        </div>
        <p className="mt-1 text-xs text-navy-600">Last run {workflow.lastRun}</p>
      </div>

      <button
        type="button"
        title="Actions"
        className="flex size-8 shrink-0 items-center justify-center rounded-md text-navy-600 transition-colors hover:bg-white/5 hover:text-ice-300"
      >
        <EllipsisIcon className="size-5" />
      </button>
    </div>
  );
}

export default function Workflows() {
  return (
    <div className="mx-auto max-w-3xl space-y-3">
      {workflows.map((workflow) => (
        <WorkflowCard key={workflow.filename} workflow={workflow} />
      ))}
    </div>
  );
}
