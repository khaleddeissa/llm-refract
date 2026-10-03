import { afterEach, expect, it, vi } from "vitest";
import {
  beginLogin,
  completeLogin,
  logout,
  type LoginConfiguration,
} from "./login";
import { request, setApiKey } from "./api";
const config: LoginConfiguration = {
  issuer: "https://identity.invalid",
  client_id: "server-client",
  redirect_uri: "https://inspector.invalid/",
};
afterEach(() => {
  vi.unstubAllGlobals();
  setApiKey("");
});
it("exchanges through same-origin endpoints without receiving or attaching provider tokens", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          url: "https://identity.invalid/authorize?state=bound",
        }),
      ),
    )
    .mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify({ authenticated: true }))),
    );
  vi.stubGlobal("fetch", fetch);
  expect(await beginLogin(config)).toContain("state=bound");
  expect(
    await completeLogin(
      config,
      new URL("https://inspector.invalid/?state=bound&code=once"),
    ),
  ).toBe(true);
  expect(fetch.mock.calls[1][0]).toBe("/v1/auth/complete");
  expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({
    state: "bound",
    code: "once",
  });
  await request("/v1/search");
  expect(fetch.mock.calls[2][1].headers.Authorization).toBeUndefined();
  await logout();
  expect(fetch.mock.calls[3][0]).toBe("/v1/auth/logout");
});
it("rejects wrong callback origins, missing state, denied login and insecure authorization URLs", async () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  await expect(
    completeLogin(config, new URL("https://evil.invalid/?code=once&state=x")),
  ).rejects.toThrow("location");
  await expect(
    completeLogin(config, new URL("https://inspector.invalid/?code=once")),
  ).rejects.toThrow("state");
  await expect(
    completeLogin(config, new URL("https://inspector.invalid/?error=denied")),
  ).rejects.toThrow("declined");
  expect(fetch).not.toHaveBeenCalled();
  fetch.mockResolvedValue(
    new Response(JSON.stringify({ url: "http://identity.invalid/authorize" })),
  );
  await expect(beginLogin(config)).rejects.toThrow("HTTPS");
});
