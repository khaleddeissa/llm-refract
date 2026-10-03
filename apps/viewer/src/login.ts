import { request } from "./api";
export interface LoginConfiguration {
  issuer: string;
  client_id: string;
  redirect_uri: string;
}
/** PKCE secrets and provider tokens remain in encrypted server storage. */
export async function beginLogin(_config: LoginConfiguration): Promise<string> {
  const result = await request<{ url: string }>("/v1/auth/start", {});
  const url = new URL(result.url);
  if (url.protocol !== "https:")
    throw new Error("SSO authorization requires HTTPS");
  return url.toString();
}
export async function completeLogin(
  config: LoginConfiguration,
  callback: URL,
): Promise<boolean> {
  if (!callback.searchParams.has("code") && !callback.searchParams.has("error"))
    return false;
  const redirect = new URL(config.redirect_uri);
  if (
    callback.origin !== redirect.origin ||
    callback.pathname !== redirect.pathname
  )
    throw new Error("Invalid SSO callback location");
  if (callback.searchParams.has("error"))
    throw new Error("Identity provider declined sign-in");
  const code = callback.searchParams.get("code");
  const state = callback.searchParams.get("state");
  if (!code || !state) throw new Error("Missing SSO callback code or state");
  await request("/v1/auth/complete", { code, state });
  return true;
}
export async function logout(): Promise<void> {
  await request("/v1/auth/logout", {});
}
