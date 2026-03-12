import type { Theme } from "./theme";

interface GraphTheme {
  fontcolor: string;
  edgeColor: string;
  nodeFill: string;
  nodeText: string;
  startFill: string;
  startBorder: string;
  startText: string;
  gateFill: string;
  gateBorder: string;
  gateText: string;
  completedFill: string;
  completedBorder: string;
  completedText: string;
  runningFill: string;
  runningBorder: string;
  runningText: string;
  runningPulseFill: string;
  runningPulseStroke: string;
}

const dark: GraphTheme = {
  fontcolor: "#5a7a94",
  edgeColor: "#2a3f52",
  nodeFill: "#1a2b3c",
  nodeText: "#c6d4e0",
  startFill: "#0d4f4f",
  startBorder: "#14b8a6",
  startText: "#5eead4",
  gateFill: "#1a2030",
  gateBorder: "#f59e0b",
  gateText: "#fbbf24",
  completedFill: "#0a2a20",
  completedBorder: "#34d399",
  completedText: "#6ee7b7",
  runningFill: "#0d3a3a",
  runningBorder: "#14b8a6",
  runningText: "#5eead4",
  runningPulseFill: "#134e4a",
  runningPulseStroke: "#5eead4",
};

const light: GraphTheme = {
  fontcolor: "#475569",
  edgeColor: "#cbd5e1",
  nodeFill: "#f1f5f9",
  nodeText: "#1e293b",
  startFill: "#ccfbf1",
  startBorder: "#0d9488",
  startText: "#0d9488",
  gateFill: "#fef3c7",
  gateBorder: "#d97706",
  gateText: "#d97706",
  completedFill: "#d1fae5",
  completedBorder: "#059669",
  completedText: "#059669",
  runningFill: "#ccfbf1",
  runningBorder: "#0d9488",
  runningText: "#0d9488",
  runningPulseFill: "#99f6e4",
  runningPulseStroke: "#0d9488",
};

export function getGraphTheme(theme: Theme): GraphTheme {
  return theme === "dark" ? dark : light;
}

