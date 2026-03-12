import { importPKCS8, SignJWT } from "jose";
import { getAppConfig } from "./lib/config.server";
import { isDemoMode } from "./lib/demo-mode.server";
import { getUser } from "./lib/session.server";

const ARC_JWT_PRIVATE_KEY = process.env.ARC_JWT_PRIVATE_KEY;

function decodePemEnv(value: string): string {
  if (value.startsWith("-----")) return value;
  return Buffer.from(value, "base64").toString("utf-8");
}

let cachedKey: CryptoKey | null = null;

async function getSigningKey(): Promise<CryptoKey> {
  if (cachedKey) return cachedKey;
  if (!ARC_JWT_PRIVATE_KEY) {
    throw new Error("ARC_JWT_PRIVATE_KEY environment variable is not set");
  }
  cachedKey = await importPKCS8(decodePemEnv(ARC_JWT_PRIVATE_KEY), "EdDSA");
  return cachedKey;
}

async function signToken(sub?: string): Promise<string> {
  const key = await getSigningKey();
  return new SignJWT({ iss: "arc-web", ...(sub ? { sub } : {}) })
    .setProtectedHeader({ alg: "EdDSA" })
    .setIssuedAt()
    .setExpirationTime("30s")
    .sign(key);
}

interface ApiOptions {
  init?: RequestInit;
  request?: Request;
}

/**
 * Fetch wrapper that signs requests with a JWT for service-to-service auth.
 * When a request is provided, the authenticated user's URL is included as
 * the JWT `sub` claim.
 */
export async function apiFetch(
  path: string,
  options?: ApiOptions
): Promise<Response> {
  const { base_url } = getAppConfig().api;
  const { init, request } = options ?? {};

  let sub: string | undefined;
  if (request) {
    const user = await getUser(request);
    sub = user?.userUrl;
  }

  const headers = new Headers(init?.headers);
  if (ARC_JWT_PRIVATE_KEY) {
    const token = await signToken(sub);
    headers.set("Authorization", `Bearer ${token}`);
  }
  if (request && isDemoMode(request)) {
    headers.set("X-Arc-Demo", "1");
  }

  const url = `${base_url}${path}`;
  try {
    return await fetch(url, { ...init, headers });
  } catch (cause) {
    throw new Error(`API request to ${url} failed`, { cause });
  }
}

/**
 * Typed JSON fetch helper. Calls apiFetch and parses the JSON response.
 */
export async function apiJson<T>(path: string, options?: ApiOptions): Promise<T> {
  const res = await apiFetch(path, options);
  if (!res.ok) throw new Response(null, { status: res.status });
  return res.json() as Promise<T>;
}
