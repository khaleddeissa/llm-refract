/** Capture real Inspector interactions against the disposable integrated demo stack. */
import { chromium, expect } from "@playwright/test";
import { readFile, mkdir, copyFile } from "node:fs/promises";
import { join } from "node:path";
const output = process.env.REFRACT_CAPTURE_DIR ?? "/tmp/refract-media";
const runs = JSON.parse(
  await readFile(
    process.env.REFRACT_SMOKE_RESULT ?? "/tmp/refract-stack-results.json",
    "utf8",
  ),
);
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({
  viewport: { width: 1600, height: 1000 },
  deviceScaleFactor: 1,
  colorScheme: "dark",
  extraHTTPHeaders: {
    Authorization: `Bearer ${process.env.REFRACT_API_KEY ?? "local-smoke-admin-key-32-characters"}`,
  },
  recordVideo: {
    dir: join(output, "video"),
    size: { width: 1600, height: 1000 },
  },
});
const page = await context.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(String(error)));
const pause = () => page.waitForTimeout(1800);
try {
  await page.goto(process.env.REFRACT_SERVER_URL ?? "http://127.0.0.1:51098");
  await page
    .getByRole("button", { name: /Returns assistant · baseline/ })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "Returns assistant · baseline",
      exact: true,
    }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Graph event Draft answer", exact: true })
    .click();
  await pause();
  await page.screenshot({
    path: join(output, "Inspector_Layout_1.PNG"),
    fullPage: true,
  });
  await page.getByLabel("Compare with run").selectOption(runs.candidate);
  await page.getByLabel("Semantic grader").selectOption("local-demo");
  await page.getByLabel("Authorize model grading calls").check();
  await page.getByRole("button", { name: "Diff", exact: true }).click();
  await expect(page.getByText(/changed events/)).toBeVisible();
  await pause();
  await page.screenshot({
    path: join(output, "Inspector_Layout_2.PNG"),
    fullPage: true,
  });
  await page.getByText("Rerun with a model", { exact: true }).click();
  await page
    .getByLabel("Generation model", { exact: true })
    .selectOption("local-demo");
  await page.getByLabel(/Authorize .* provider calls/).check();
  await pause();
  await page.screenshot({
    path: join(output, "Inspector_Layout_3.PNG"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Create model branch" }).click();
  await expect(
    page.getByRole("heading", { name: /rerun|branch/ }),
  ).toBeVisible();
  await pause();
  await page
    .getByLabel("Search mode", { exact: true })
    .selectOption("local-demo");
  await page.getByLabel("Search executions").fill("return policy");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByText("Project embeddings", { exact: true }).click();
  await pause();
  await page.screenshot({
    path: join(output, "Inspector_Search.PNG"),
    fullPage: true,
  });
  await page
    .getByText("OpenTelemetry logs and metrics", { exact: true })
    .click();
  await page
    .getByRole("button", { name: "Load telemetry", exact: true })
    .click();
  await page.locator(".telemetry-panel details summary").first().click();
  await page.locator(".telemetry-panel").scrollIntoViewIfNeeded();
  await pause();
  await page.screenshot({
    path: join(output, "Inspector_Telemetry.PNG"),
    fullPage: true,
  });
  await page.getByLabel("Telemetry signal").selectOption("metrics");
  await page
    .getByRole("button", { name: "Load telemetry", exact: true })
    .click();
  await page.locator(".telemetry-panel details summary").first().click();
  await pause();
  if (errors.length) throw new Error(errors.join("\n"));
} finally {
  const video = page.video();
  await context.close();
  if (video)
    await copyFile(await video.path(), join(output, "Inspector_Demo.webm"));
  await browser.close();
}
console.log("Saved current Inspector screenshots and recorded WebM to", output);
