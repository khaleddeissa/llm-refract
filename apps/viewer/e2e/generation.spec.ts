import { test, expect } from "@playwright/test";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };

test("model rerun and domain grading require explicit consent", async ({
  page,
}) => {
  const candidate = { ...fixture, id: "candidate", name: "Candidate response" };
  await page.route("**/v1/search?*", (route) =>
    route.fulfill({ json: { runs: [fixture, candidate], total: 2 } }),
  );
  await page.route("**/v1/generation-models", (route) =>
    route.fulfill({
      json: {
        models: [
          {
            id: "local",
            label: "Local model",
            model: "fixture",
            grading: true,
          },
        ],
      },
    }),
  );
  await page.route("**/v1/runs/*/rerun", async (route) => {
    expect(route.request().postDataJSON()).toMatchObject({
      profile: "local",
      from_event: fixture.events[0].id,
      allow_live: true,
      reuse_recorded: [fixture.events[0].id],
    });
    await route.fulfill({
      status: 201,
      json: { ...fixture, id: "branch", name: "Model branch" },
    });
  });
  await page.route("**/v1/diff", async (route) => {
    expect(route.request().postDataJSON()).toMatchObject({
      grader: "local",
      allow_live: true,
    });
    await route.fulfill({
      json: {
        semantic_report: {
          passed: true,
          equivalent: 2,
          changed: 0,
          differences: [],
          budget_violations: [],
        },
      },
    });
  });
  await page.goto("/");
  await page.getByText("Rerun with a model", { exact: true }).click();
  await page
    .getByLabel("Generation model", { exact: true })
    .selectOption("local");
  const run = page.getByRole("button", { name: "Create model branch" });
  await expect(run).toBeDisabled();
  await page.getByLabel(/Authorize .* provider calls/).check();
  await expect(run).toBeDisabled();
  await page.getByLabel(/Reuse recorded outputs/).check();
  await run.click();
  await expect(
    page.getByRole("heading", { name: "Model branch", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Compare with run").selectOption("candidate");
  await page.getByLabel("Semantic grader").selectOption("local");
  await expect(
    page.getByRole("button", { name: "Diff", exact: true }),
  ).toBeDisabled();
  await page.getByLabel("Authorize model grading calls").check();
  await page.getByRole("button", { name: "Diff", exact: true }).click();
  await expect(
    page.getByText("2 equivalent events · 0 changed events"),
  ).toBeVisible();
});
