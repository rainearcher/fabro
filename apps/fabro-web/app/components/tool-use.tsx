import { useState } from "react";
import { ChevronRightIcon } from "@heroicons/react/20/solid";
import { WrenchScrewdriverIcon } from "@heroicons/react/24/outline";

export interface ToolUse {
  id: string;
  toolName: string;
  input: string;
  result: string;
  isError: boolean;
  durationMs?: number;
}

export function ToolRow({ tool }: { tool: ToolUse }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="border-b border-line last:border-b-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left transition-colors hover:bg-overlay"
      >
        <ChevronRightIcon className={`size-3 shrink-0 text-fg-muted transition-transform duration-150 ${open ? "rotate-90" : ""}`} />
        <WrenchScrewdriverIcon className="size-3.5 shrink-0 text-fg-muted" />
        <span className="font-mono text-xs text-fg-3">{tool.toolName}</span>
        {tool.durationMs != null && <span className="text-[11px] text-fg-muted">{tool.durationMs}ms</span>}
        <span className="truncate font-mono text-xs text-fg-muted">{tool.input}</span>
      </button>
      {open && (
        <div className="space-y-px bg-overlay px-2.5 pb-2 pt-1">
          <div className="rounded bg-overlay px-2.5 py-2">
            <div className="mb-1 text-[10px] font-medium uppercase tracking-wider text-fg-muted">Input</div>
            <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-fg-3">{tool.input}</pre>
          </div>
          <div className="rounded bg-overlay px-2.5 py-2">
            <div className="mb-1 text-[10px] font-medium uppercase tracking-wider text-fg-muted">Result</div>
            <pre className={`whitespace-pre-wrap font-mono text-xs leading-relaxed ${tool.isError ? "text-coral" : "text-fg-3"}`}>{tool.result}</pre>
          </div>
        </div>
      )}
    </div>
  );
}

export function ToolBlock({ tools }: { tools: ToolUse[] }) {
  return (
    <div className="rounded-md border border-line bg-overlay overflow-hidden">
      {tools.map((tool) => (
        <ToolRow key={tool.id} tool={tool} />
      ))}
    </div>
  );
}
