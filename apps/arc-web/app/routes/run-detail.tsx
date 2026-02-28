import { ChevronRightIcon } from "@heroicons/react/20/solid";
import { Link, Outlet, useLocation } from "react-router";
import { findRun, statusColors, ciConfig } from "../data/runs";
import { workflowData } from "./workflow-detail";
import type { Route } from "./+types/run-detail";

const tabs = [
  { name: "Overview", path: "" },
  { name: "Stages", path: "/stages" },
  { name: "Files Changed", path: "/files" },
];

export const handle = { hideHeader: true };

export function meta({ params }: Route.MetaArgs) {
  const run = findRun(params.id);
  return [{ title: run ? `${run.title} — Arc` : "Run — Arc" }];
}

export default function RunDetail({ params }: Route.ComponentProps) {
  const run = findRun(params.id);
  const { pathname } = useLocation();
  const basePath = `/runs/${params.id}`;

  if (!run) {
    return <p className="py-8 text-center text-sm text-navy-600">Run not found.</p>;
  }

  const colors = statusColors[run.status];

  return (
    <div>
      <nav className="mb-4 flex items-center gap-1 text-sm text-navy-600">
        <Link to="/runs" className="text-ice-300 hover:text-white">Runs</Link>
        <ChevronRightIcon className="size-3" />
        <Link to={`/workflows/${run.workflow}`} className="text-ice-300 hover:text-white">
          {workflowData[run.workflow]?.title ?? run.workflow}
        </Link>
        <ChevronRightIcon className="size-3" />
        <span>{run.title}</span>
      </nav>

      <div className="mb-6">
        <h2 className="text-xl font-semibold text-white">{run.title}</h2>
        <div className="mt-2 flex items-center gap-3 text-sm">
          <span className="flex items-center gap-1.5">
            <span className={`size-2 rounded-full ${colors.dot}`} />
            <span className={`font-medium ${colors.text}`}>{run.statusLabel}</span>
          </span>
          <span className="font-mono text-xs text-navy-600">{run.repo}</span>
          {run.elapsed && (
            <span className={`font-mono text-xs ${run.elapsedWarning ? "text-amber" : "text-navy-600"}`}>{run.elapsed}</span>
          )}
          {run.number != null && (
            <span className="font-mono text-xs text-navy-600">#{run.number}</span>
          )}
          {run.ci && (
            <span className={`flex items-center gap-1 font-mono text-xs ${ciConfig[run.ci].text}`}>
              <span className={`size-1.5 rounded-full ${ciConfig[run.ci].dot}`} />
              {ciConfig[run.ci].label}
            </span>
          )}
        </div>
      </div>

      <div className="border-b border-white/[0.06]">
        <nav className="-mb-px flex gap-6">
          {tabs.map((tab) => {
            const tabPath = `${basePath}${tab.path}`;
            const isActive = pathname === tabPath;
            return (
              <Link
                key={tab.name}
                to={tabPath}
                className={`border-b-2 pb-3 text-sm font-medium transition-colors ${
                  isActive
                    ? "border-teal-500 text-white"
                    : "border-transparent text-navy-600 hover:border-white/10 hover:text-ice-300"
                }`}
              >
                {tab.name}
              </Link>
            );
          })}
        </nav>
      </div>

      <div className="mt-6">
        <Outlet />
      </div>
    </div>
  );
}
