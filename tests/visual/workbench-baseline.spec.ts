import { expect, test } from "@playwright/test";

const THEMES = ["dark", "light", "high-contrast"] as const;

for (const theme of THEMES) {
  test(`empty workbench · ${theme}`, async ({ page }) => {
    await page.addInitScript((selectedTheme) => {
      localStorage.setItem("sunsetz.theme", selectedTheme);
    }, theme);

    await page.goto("/");
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await expect(page.getByRole("heading", { name: "新会话" })).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "我们今天要一起做些什么？" }),
    ).toBeVisible();

    const geometry = await page.evaluate(() => {
      const composer = document.querySelector(".composer");
      const main = document.querySelector("main.main");
      const projectRail = document.querySelector(".composer-context-rail");
      const projectTrigger = projectRail?.querySelector(".chip--project");
      const composerRect = composer?.getBoundingClientRect();
      const mainRect = main?.getBoundingClientRect();
      return {
        horizontalOverflow:
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
        composerWidth: composerRect?.width ?? 0,
        composerLeft: composerRect?.left ?? 0,
        composerRight: composerRect?.right ?? 0,
        mainWidth: mainRect?.width ?? 0,
        mainLeft: mainRect?.left ?? 0,
        mainRight: mainRect?.right ?? 0,
        projectRailWidth: projectRail?.getBoundingClientRect().width ?? 0,
        projectTriggerWidth:
          projectTrigger?.getBoundingClientRect().width ?? 0,
      };
    });

    expect(geometry.horizontalOverflow).toBeLessThanOrEqual(0);
    expect(geometry.composerWidth).toBeGreaterThan(300);
    expect(geometry.mainWidth).toBeGreaterThan(360);
    expect(geometry.composerLeft).toBeGreaterThanOrEqual(
      geometry.mainLeft - 1,
    );
    expect(geometry.composerRight).toBeLessThanOrEqual(
      geometry.mainRight + 1,
    );
    expect(geometry.projectTriggerWidth).toBeLessThan(
      geometry.projectRailWidth,
    );
    await expect(page).toHaveScreenshot(`${theme}-empty-workbench.png`, {
      animations: "disabled",
      caret: "hide",
      fullPage: false,
      // Chromium rasterizes the app window's 12 px bottom corner differently
      // across otherwise identical runs. Keep this below one small corner's
      // footprint so content and layout changes still fail the baseline.
      maxDiffPixels: 100,
    });
  });
}

test("context summary stays compact and anchored", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("sunsetz.theme", "dark");
  });
  await page.goto("/");

  const trigger = page.getByRole("button", { name: /上下文用量/ });
  await trigger.hover();
  const summary = page.getByRole("tooltip");
  await expect(summary).toBeVisible();

  const geometry = await summary.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      width: rect.width,
      top: rect.top,
      right: rect.right,
      viewportWidth: window.innerWidth,
    };
  });
  expect(geometry.width).toBeGreaterThanOrEqual(150);
  expect(geometry.width).toBeLessThanOrEqual(220);
  expect(geometry.top).toBeGreaterThanOrEqual(0);
  expect(geometry.right).toBeLessThanOrEqual(geometry.viewportWidth);

  await expect(page).toHaveScreenshot("dark-context-summary.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });
});

test("active plan mode sits beside access and closes in place", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("sunsetz.theme", "dark");
  });
  await page.goto("/");

  await page.evaluate(() => {
    const internals = {
      invoke: async (command: string, args: Record<string, unknown>) => {
        if (command === "composer_prefs_set") {
          return {
            modelId: "sunsetz-4.5",
            effort: "medium",
            mode: args.mode ?? "agent",
            permissionPolicy: "ask",
            scope: "global",
            source: "visual-test",
          };
        }
        return null;
      },
    };
    Object.assign(window, { __TAURI_INTERNALS__: internals });
  });

  await page.getByRole("button", { name: "添加", exact: true }).click();
  const addMenu = page.getByRole("menu");
  await expect(addMenu).toBeVisible();
  await addMenu.getByRole("menuitem", { name: /计划模式/ }).click();
  await expect(addMenu).toHaveCount(0);

  const planButton = page.getByRole("button", { name: "计划模式" });
  await expect(planButton).toBeVisible();
  await expect(page.locator(".composer-context-rail__activity")).toHaveCount(0);
  await planButton.hover();

  await expect(page).toHaveScreenshot("dark-plan-mode.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });

  await planButton.click();
  await expect(planButton).toHaveCount(0);
});
