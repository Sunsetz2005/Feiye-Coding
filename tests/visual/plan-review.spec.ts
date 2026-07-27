import { expect, test } from "@playwright/test";

test("plan review has one decision surface and a read-only resource panel", async ({
  page,
}) => {
  await page.goto("/tests/visual/fixtures/plan-review.html");

  await expect(page.getByText("计划待审阅", { exact: true })).toHaveCount(1);
  await expect(
    page.getByRole("button", { name: /批准并构建/ }),
  ).toHaveCount(1);

  const resourcePlan = page.getByTestId("plan-review-panel");
  await expect(resourcePlan).toBeVisible();
  await expect(resourcePlan.getByText("计划", { exact: true })).toBeVisible();
  await expect(resourcePlan.getByRole("button")).toHaveCount(2);
  await expect(resourcePlan).not.toContainText("批准并构建");
  await expect(resourcePlan).not.toContainText("计划待审阅");

  await expect(page).toHaveScreenshot("plan-review-single-decision.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });
});
