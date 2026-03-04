import { redirect } from "react-router";
import { getAppConfig } from "../lib/config.server";
import { isGitHubAppConfigured } from "../lib/github.server";
import { getUser } from "../lib/session.server";
import type { Route } from "./+types/redirect-home";

export async function loader({ request }: Route.LoaderArgs) {
  const { provider } = getAppConfig().web.auth;
  if (provider === "github" && !isGitHubAppConfigured()) {
    return redirect("/setup");
  }
  if (provider !== "insecure_disabled") {
    const user = await getUser(request);
    if (!user) {
      return redirect("/auth/login");
    }
  }
  return redirect("/start");
}
