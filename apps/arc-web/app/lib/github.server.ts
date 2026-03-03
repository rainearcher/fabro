import { GitHub, generateState } from "arctic";
import { getAppConfig } from "./config.server";

export { generateState };

export function getGitHubOAuth() {
  const clientId = getAppConfig().git.client_id;
  const clientSecret = process.env.GITHUB_APP_CLIENT_SECRET;
  if (!clientId || !clientSecret) {
    throw new Error("GitHub App is not configured");
  }
  return new GitHub(clientId, clientSecret, null);
}

export function isGitHubAppConfigured(): boolean {
  return getAppConfig().git.client_id !== null;
}

export function getGitHubAppPrivateKey(): string {
  const raw = process.env.GITHUB_APP_PRIVATE_KEY;
  if (!raw) {
    throw new Error("GITHUB_APP_PRIVATE_KEY is not configured");
  }
  if (raw.startsWith("-----BEGIN")) {
    return raw;
  }
  return Buffer.from(raw, "base64").toString("utf-8");
}
