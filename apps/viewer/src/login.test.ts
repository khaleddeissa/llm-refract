import { afterEach, expect, it, vi } from "vitest";
import { beginLogin, completeLogin, type LoginConfiguration } from "./login";
import { request, setApiKey } from "./api";
const config: LoginConfiguration = {
  issuer: "https://identity.invalid",
  client_id: "public-client",
  scope: "openid refract.read",
  authorization_endpoint: "https://identity.invalid/authorize",
  token_endpoint: "https://identity.invalid/token",
  redirect_uri: "https://inspector.invalid/",
  authorization_params: { audience: "refract" },
};
function storage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    key: (i) => [...entries.keys()][i] ?? null,
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => {
      entries.set(key, value);
    },
    removeItem: (key) => {
      entries.delete(key);
    },
    clear: () => {
      entries.clear();
    },
  };
}
afterEach(() => {
  vi.unstubAllGlobals();
  setApiKey("");
});
it("uses S256 PKCE and retains only pending state, then keeps tokens in memory", async () => {
  const session = storage();
  const authorization = new URL(await beginLogin(config, session));
  expect(authorization.searchParams.get("code_challenge_method")).toBe("S256");
  expect(authorization.searchParams.get("code_challenge")).toHaveLength(43);
  expect(authorization.searchParams.get("response_type")).toBe("code");
  const fetch = vi.fn().mockImplementation(() =>
    Promise.resolve(
      new Response(
        JSON.stringify({
          access_token: "fixture-access",
          token_type: "Bearer",
        }),
      ),
    ),
  );
  vi.stubGlobal("fetch", fetch);
  const callback = new URL(config.redirect_uri);
  callback.searchParams.set("code", "fixture-code");
  callback.searchParams.set("state", authorization.searchParams.get("state")!);
  expect(await completeLogin(config, callback, session)).toBe(true);
  expect(session.length).toBe(0);
  expect(fetch.mock.calls[0][1].body.get("code_verifier")).toHaveLength(43);
  expect(fetch.mock.calls[0][1].redirect).toBe("error");
  await request("/v1/search");
  expect(fetch.mock.calls[1][1].headers.Authorization).toBe(
    "Bearer fixture-access",
  );
  await expect(completeLogin(config, callback, session)).rejects.toThrow(
    "state is missing",
  );
});
it("rejects forged state and redirects before sending a token request", async () => {
  const session = storage();
  await beginLogin(config, session);
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  await expect(
    completeLogin(
      config,
      new URL("https://inspector.invalid/?code=stolen&state=forged"),
      session,
    ),
  ).rejects.toThrow("invalid or expired");
  expect(fetch).not.toHaveBeenCalled();
  expect(session.length).toBe(0);
});
