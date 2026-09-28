import { test, expect } from "@playwright/test";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };
test("inspect, replay, and fork a recorded run", async ({
  page,
  request,
}, testInfo) => {
  const response = await request.post("/v1/runs", { data: fixture });
  expect([201, 409]).toContain(response.status());
  await page.goto("/");
  await page.getByRole("button", { name: /customer-support.*demo-1/ }).click();
  await expect(
    page.getByRole("heading", { name: "customer-support" }),
  ).toBeVisible();
  await page.getByRole("button", { name: /02 Draft answer/ }).click();
  await expect(
    page.getByRole("heading", { name: "Draft answer" }),
  ).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath("execution.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: /Replay recorded/ }).click();
  await expect(
    page.getByRole("heading", { name: "Execution result" }),
  ).toBeVisible();
  await page.getByRole("button", { name: /Fork before event/ }).click();
  await expect(page.getByText("1 steps", { exact: true })).toBeVisible();
});

test("artifact download is readable and replay rejects live execution", async ({
  request,
}) => {
  const created = await request.post("/v1/runs", { data: fixture });
  expect([201, 409]).toContain(created.status());
  const artifact = await request.get("/v1/runs/demo-1/artifact");
  expect(artifact.ok()).toBeTruthy();
  const text = await artifact.text();
  const split = text.indexOf("\n");
  expect(JSON.parse(text.slice(0, split)).format).toBe("refract.artifact.v1");
  expect(JSON.parse(text.slice(split + 1)).events).toHaveLength(2);
  const live = await request.post("/v1/runs/demo-1/replay", {
    data: { mode: "live" },
  });
  expect(live.status()).toBe(400);
});

test("search, causal graph selection, metrics and comparison", async ({
  page,
  request,
}) => {
  const baseline = structuredClone(fixture);
  baseline.id = `graph-base-${Date.now()}`;
  baseline.name = "Graph baseline";
  baseline.events.forEach((event) => {
    event.run_id = baseline.id;
  });
  baseline.events[1].parent_id = baseline.events[0].id as never;
  baseline.events[1].attributes = {
    model: "graph-model",
    input_tokens: 100,
    output_tokens: 20,
    cost_usd: 0.01,
    ttft_ms: 25,
  } as never;
  const candidate = structuredClone(baseline);
  candidate.id = `${baseline.id}-candidate`;
  candidate.name = "Graph candidate";
  candidate.events.forEach((event) => {
    event.run_id = candidate.id;
  });
  candidate.events[1].attributes = {
    model: "graph-model",
    input_tokens: 50,
    output_tokens: 10,
    cost_usd: 0.005,
    ttft_ms: 12,
  } as never;
  expect(
    (await request.post("/v1/runs", { data: baseline })).ok(),
  ).toBeTruthy();
  expect(
    (await request.post("/v1/runs", { data: candidate })).ok(),
  ).toBeTruthy();
  await page.goto("/");
  await page.getByLabel("Search executions").fill("Graph");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page
    .getByRole("button", {
      name: new RegExp(`Graph baseline.*${baseline.id.slice(0, 20)}`),
    })
    .click();
  await expect(
    page.getByRole("region", { name: "Execution metrics" }),
  ).toContainText("120");
  await page
    .getByRole("button", { name: "Graph event Draft answer", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Draft answer", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Compare with run").selectOption(candidate.id);
  await expect(
    page.getByRole("region", { name: "Metric comparison" }),
  ).toContainText("-50.0%");
  await page.getByRole("button", { name: "Diff", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Semantic comparison results" }),
  ).toContainText("Comparison passed");
});

test("API key enables authenticated search and is forgotten on reload", async ({
  page,
}) => {
  await page.route("**/v1/search?**", async (route) => {
    const authorized =
      route.request().headers().authorization === "Bearer test-viewer-key";
    await route.fulfill({
      status: authorized ? 200 : 401,
      contentType: "application/json",
      body: JSON.stringify(
        authorized
          ? { runs: [fixture], total: 1, limit: 100, offset: 0 }
          : { error: "API key required" },
      ),
    });
  });
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText("401");
  await page
    .getByLabel("API key or access token (tab memory only)")
    .fill("test-viewer-key");
  await page.getByRole("button", { name: "Use API key", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "customer-support" }),
  ).toBeVisible();
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("401");
});

test("SSO exchanges a PKCE code and forgets the access token on reload", async ({
  page,
  baseURL,
}) => {
  const redirect = new URL("/", baseURL!).toString();
  let challenge = "";
  let exchanged = false;
  await page.route("**/v1/auth/config", (route) =>
    route.fulfill({
      json: {
        enabled: true,
        configuration: {
          issuer: "https://identity.invalid",
          client_id: "fixture-inspector",
          authorization_endpoint: "https://identity.invalid/authorize",
          token_endpoint: "https://identity.invalid/token",
          redirect_uri: redirect,
          scope: "openid",
          authorization_params: {},
        },
      },
    }),
  );
  await page.route("https://identity.invalid/authorize?**", async (route) => {
    const url = new URL(route.request().url());
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
    challenge = url.searchParams.get("code_challenge")!;
    const callback = new URL(redirect);
    callback.searchParams.set("code", "local-once");
    callback.searchParams.set("state", url.searchParams.get("state")!);
    await route.fulfill({
      status: 302,
      headers: { location: callback.toString() },
    });
  });
  await page.route("https://identity.invalid/token", async (route) => {
    const { createHash } = await import("node:crypto");
    const body = new URLSearchParams(route.request().postData()!);
    expect(body.get("code")).toBe("local-once");
    expect(
      createHash("sha256")
        .update(body.get("code_verifier")!)
        .digest("base64url"),
    ).toBe(challenge);
    expect(exchanged).toBe(false);
    exchanged = true;
    await route.fulfill({
      headers: { "access-control-allow-origin": new URL(redirect).origin },
      json: { access_token: "fixture-access-token", token_type: "Bearer" },
    });
  });
  await page.route("**/v1/search?**", (route) => {
    const authorized =
      route.request().headers().authorization === "Bearer fixture-access-token";
    return route.fulfill({
      status: authorized ? 200 : 401,
      json: authorized
        ? { runs: [fixture], total: 1, limit: 100, offset: 0 }
        : { error: "sign in" },
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Sign in with SSO" }).click();
  await expect(
    page.getByRole("heading", { name: "customer-support" }),
  ).toBeVisible();
  expect(exchanged).toBe(true);
  expect(page.url()).toBe(redirect);
  expect(
    await page.evaluate(() => sessionStorage.getItem("refract.oidc.pending")),
  ).toBeNull();
  expect(
    await page.evaluate(() =>
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    ),
  ).not.toContain("fixture-access-token");
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("401");
});

test("late startup refresh preserves the run selected while authentication config loads", async ({
  page,
}) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/v1/auth/config", async (route) => {
    await gate;
    await route.fulfill({ json: { enabled: false } });
  });
  const first = { ...fixture, id: "refresh-first", name: "Refresh first" };
  const second = {
    ...fixture,
    id: "refresh-selected",
    name: "Refresh selected",
  };
  await page.route("**/v1/search?**", (route) =>
    route.fulfill({ json: { runs: [first, second], total: 2 } }),
  );
  await page.goto("/");
  await page
    .getByLabel("API key or access token (tab memory only)")
    .fill("fixture");
  await page.getByRole("button", { name: "Use API key", exact: true }).click();
  await page
    .getByRole("button", { name: /Refresh selected.*refresh-selected/ })
    .click();
  const refreshed = page.waitForResponse((response) =>
    response.url().includes("/v1/search?"),
  );
  release();
  await refreshed;
  await expect(
    page.getByRole("heading", { name: "Refresh selected", exact: true }),
  ).toBeVisible();
});
