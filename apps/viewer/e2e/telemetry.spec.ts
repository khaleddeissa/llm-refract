import { test, expect } from "@playwright/test";
test("Inspector reads logs and metrics and filters trace-correlated logs", async ({
  page,
}) => {
  const trace = "0123456789abcdef0123456789abcdef";
  await page.route("**/v1/telemetry?**", (route) => {
    const params = new URL(route.request().url()).searchParams;
    const kind = params.get("kind");
    if (params.get("trace_id")) expect(params.get("trace_id")).toBe(trace);
    return route.fulfill({
      json: {
        next_offset: 1,
        records:
          kind === "metrics"
            ? [
                {
                  kind,
                  trace_id: "",
                  payload: {
                    metric: {
                      name: "example.requests",
                      sum: { dataPoints: [{ asInt: "1" }] },
                    },
                  },
                },
              ]
            : [
                {
                  kind,
                  trace_id: trace,
                  payload: {
                    severity_text: "INFO",
                    body: "Local generation completed",
                    attributes: { api_key: "[REDACTED]" },
                  },
                },
              ],
      },
    });
  });
  await page.goto("/");
  await page
    .getByText("OpenTelemetry logs and metrics", { exact: true })
    .click();
  await page.getByLabel("Telemetry trace ID").fill(trace);
  await page
    .getByRole("button", { name: "Load telemetry", exact: true })
    .click();
  await page.getByText(`INFO · ${trace}`, { exact: true }).click();
  await expect(page.getByText(/Local generation completed/)).toBeVisible();
  await page.getByLabel("Telemetry signal").selectOption("metrics");
  await expect(page.getByText("0 telemetry records loaded")).toBeVisible();
  await page
    .getByRole("button", { name: "Load telemetry", exact: true })
    .click();
  await expect(
    page.getByText("example.requests", { exact: true }),
  ).toBeVisible();
});
