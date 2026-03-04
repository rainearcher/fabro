import { redirect } from "react-router";
import { getAppConfig } from "../lib/config.server";
import { getSession, destroySession } from "../lib/session.server";
import type { Route } from "./+types/auth-logout";

export async function action({ request }: Route.ActionArgs) {
  const { provider } = getAppConfig().web.auth;
  if (provider === "tailscale") {
    return redirect("/start");
  }
  const session = await getSession(request);
  return redirect("/auth/login", {
    headers: { "Set-Cookie": await destroySession(session) },
  });
}
