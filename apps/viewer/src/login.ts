import { setApiKey } from "./api";
export interface LoginConfiguration {
  issuer: string;
  client_id: string;
  authorization_endpoint: string;
  token_endpoint: string;
  redirect_uri: string;
  scope: string;
  authorization_params: Record<string, string>;
}
const slot = "refract.oidc.pending";
function base64url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}
export async function beginLogin(
  config: LoginConfiguration,
  storage: Storage = sessionStorage,
): Promise<string> {
  const state = base64url(crypto.getRandomValues(new Uint8Array(32)));
  const verifier = base64url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = base64url(
    new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)),
    ),
  );
  const url = new URL(config.authorization_endpoint);
  if (url.protocol !== "https:")
    throw new Error("SSO authorization requires HTTPS");
  for (const [key, value] of Object.entries(config.authorization_params))
    if (["audience", "resource"].includes(key))
      url.searchParams.set(key, value);
  for (const [key, value] of Object.entries({
    response_type: "code",
    client_id: config.client_id,
    redirect_uri: config.redirect_uri,
    scope: config.scope,
    state,
    code_challenge: challenge,
    code_challenge_method: "S256",
  }))
    url.searchParams.set(key, value);
  storage.setItem(
    slot,
    JSON.stringify({
      state,
      verifier,
      created: Date.now(),
      issuer: config.issuer,
      clientId: config.client_id,
      redirect: config.redirect_uri,
    }),
  );
  return url.toString();
}
export async function completeLogin(
  config: LoginConfiguration,
  callback: URL,
  storage: Storage = sessionStorage,
): Promise<boolean> {
  if (!callback.searchParams.has("code") && !callback.searchParams.has("error"))
    return false;
  const stored = storage.getItem(slot);
  storage.removeItem(slot);
  if (!stored)
    throw new Error("SSO login state is missing; start sign-in again");
  const pending = JSON.parse(stored) as Record<string, unknown>;
  const redirect = new URL(config.redirect_uri);
  if (
    callback.origin !== redirect.origin ||
    callback.pathname !== redirect.pathname ||
    !callback.searchParams.get("state") ||
    callback.searchParams.get("state") !== pending.state ||
    pending.issuer !== config.issuer ||
    pending.clientId !== config.client_id ||
    pending.redirect !== config.redirect_uri ||
    typeof pending.created !== "number" ||
    Date.now() - pending.created > 600_000 ||
    pending.created > Date.now() ||
    typeof pending.verifier !== "string"
  )
    throw new Error("SSO login state is invalid or expired");
  if (callback.searchParams.has("error"))
    throw new Error("Identity provider declined sign-in");
  const endpoint = new URL(config.token_endpoint);
  if (endpoint.protocol !== "https:")
    throw new Error("SSO token exchange requires HTTPS");
  const response = await fetch(endpoint, {
    method: "POST",
    credentials: "omit",
    redirect: "error",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      code: callback.searchParams.get("code")!,
      client_id: config.client_id,
      redirect_uri: config.redirect_uri,
      code_verifier: pending.verifier,
    }),
  });
  if (!response.ok) throw new Error("SSO token exchange failed");
  const tokens = (await response.json()) as Record<string, unknown>;
  if (
    typeof tokens.access_token !== "string" ||
    !tokens.access_token ||
    tokens.access_token.length > 16384 ||
    typeof tokens.token_type !== "string" ||
    tokens.token_type.toLowerCase() !== "bearer"
  )
    throw new Error(
      "Identity provider did not return a supported access token",
    );
  setApiKey(tokens.access_token);
  return true;
}
