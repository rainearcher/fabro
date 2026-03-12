import { apiJson } from "../api-client";
import { CollapsibleFile } from "../components/collapsible-file";
import type { ServerConfiguration } from "@qltysh/fabro-api-client";
import type { Route } from "./+types/settings";

export function meta({}: Route.MetaArgs) {
  return [{ title: "Settings — Fabro" }];
}

export const handle = { hideHeader: true };

export async function loader({ request }: Route.LoaderArgs) {
  const config = await apiJson<ServerConfiguration>("/settings", { request });
  return { config };
}

export default function Settings({ loaderData }: Route.ComponentProps) {
  const { config } = loaderData;

  return (
    <div className="mx-auto max-w-4xl">
      <CollapsibleFile
        file={{ name: "server.json", contents: JSON.stringify(config, null, 2), lang: "json" }}
      />
    </div>
  );
}
