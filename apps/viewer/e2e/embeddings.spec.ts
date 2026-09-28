import { test, expect } from "@playwright/test";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };

test("project admins enable a model and search by meaning", async ({
  page,
}) => {
  let profiles: {
    profile: string;
    is_default: boolean;
    auto_index: boolean;
  }[] = [];
  await page.route("**/v1/embedding-models", (route) =>
    route.fulfill({
      json: {
        models: [
          {
            id: "local",
            label: "Local model",
            model: "fixture",
            dimensions: 2,
          },
        ],
      },
    }),
  );
  await page.route("**/v1/auth/me", (route) =>
    route.fulfill({ json: { role: "admin" } }),
  );
  await page.route("**/v1/project/embeddings", (route) =>
    route.fulfill({
      json: { profiles, jobs: { done: 1, pending: 0, failed: 0 } },
    }),
  );
  await page.route("**/v1/admin/project/embeddings", async (route) => {
    expect(route.request().method()).toBe("PUT");
    profiles = route.request().postDataJSON();
    await route.fulfill({ json: { profiles } });
  });
  await page.route("**/v1/search/text", async (route) => {
    expect(route.request().postDataJSON()).toMatchObject({
      query: "refund request",
      profile: "local",
    });
    await route.fulfill({
      json: {
        runs: [fixture],
        total: 1,
        matches: [{ run_id: fixture.id, score: 1 }],
      },
    });
  });
  await page.goto("/");
  await page.getByText("Project embeddings", { exact: true }).click();
  await page.getByLabel("Local model", { exact: true }).check();
  await page.getByRole("button", { name: "Save embedding settings" }).click();
  await expect(page.getByRole("status")).toContainText(
    "Embedding settings saved",
  );
  await page.getByLabel("Search mode", { exact: true }).selectOption("local");
  await page.getByLabel("Search executions").fill("refund request");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: fixture.name, exact: true }),
  ).toBeVisible();
  await expect(page.getByText("1 matching runs")).toBeVisible();
});
