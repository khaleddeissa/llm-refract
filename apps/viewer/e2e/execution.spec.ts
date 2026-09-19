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
