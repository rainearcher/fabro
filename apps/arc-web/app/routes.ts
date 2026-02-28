import {
  type RouteConfig,
  index,
  layout,
  route,
} from "@react-router/dev/routes";

export default [
  index("routes/redirect-home.tsx"),
  layout("layouts/app-shell.tsx", [
    route("start", "routes/start.tsx"),
    route("pipelines", "routes/pipelines.tsx"),
    route("insights", "routes/insights.tsx"),
    route("settings", "routes/settings.tsx"),
  ]),
] satisfies RouteConfig;
