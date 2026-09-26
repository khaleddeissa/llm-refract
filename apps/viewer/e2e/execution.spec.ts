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
  await page.getByLabel("API key (tab memory only)").fill("test-viewer-key");
  await page.getByRole("button", { name: "Use API key", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "customer-support" }),
  ).toBeVisible();
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("401");
});
