import type { Route } from "./+types/insights";

export function meta({}: Route.MetaArgs) {
  return [{ title: "Insights — Arc" }];
}

export default function Insights() {
  return null;
}
