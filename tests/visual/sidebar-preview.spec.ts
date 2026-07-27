import { expect, test } from "@playwright/test";

test("task preview is delayed, bounded, and stays inside the viewport", async ({
  page,
}) => {
  await page.goto("/tests/visual/fixtures/sidebar-preview.html");

  const task = page.getByRole("button", { name: "优化工作台侧栏预览" });
  await task.hover();

  const preview = page.locator('.sidebar-preview[data-kind="session"]');
  await expect(preview).toBeVisible();
  await expect(preview).toContainText("最近可见消息");

  const geometry = await preview.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
      width: rect.width,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    };
  });
  expect(geometry.left).toBeGreaterThanOrEqual(8);
  expect(geometry.right).toBeLessThanOrEqual(geometry.viewportWidth - 8);
  expect(geometry.top).toBeGreaterThanOrEqual(8);
  expect(geometry.bottom).toBeLessThanOrEqual(geometry.viewportHeight);
  expect(geometry.width).toBeLessThanOrEqual(310);

  await expect(page).toHaveScreenshot("sidebar-task-preview.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });
});

test("project preview is available from keyboard focus", async ({ page }) => {
  await page.goto("/tests/visual/fixtures/sidebar-preview.html");

  await page.getByRole("button", { name: "Sunsetz" }).focus();
  const preview = page.locator('.sidebar-preview[data-kind="project"]');
  await expect(preview).toBeVisible();
  await expect(preview).toContainText("2 个任务");
  await expect(preview).toContainText("/Users/Shared/Coding/Sunsetz");

  await expect(page).toHaveScreenshot("sidebar-project-preview.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });
});
