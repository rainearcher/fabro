const COOKIE_NAME = "fabro-demo";

/** Check whether demo mode is active for this request (cookie, then env var fallback). */
export function isDemoMode(request: Request): boolean {
  const cookies = request.headers.get("Cookie") ?? "";
  const match = cookies.match(/(?:^|;\s*)fabro-demo=([^;]*)/);
  if (match) return match[1] === "1";
  return process.env.ARC_DEMO === "1";
}

/** Build a Set-Cookie header value to persist the demo mode preference. */
export function demoCookieHeader(enabled: boolean): string {
  const value = enabled ? "1" : "0";
  return `${COOKIE_NAME}=${value}; Path=/; SameSite=Lax; Max-Age=${60 * 60 * 24 * 365}`;
}
