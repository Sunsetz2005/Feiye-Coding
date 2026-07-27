import { expect, test } from "@playwright/test";

const THEMES = ["dark", "light", "high-contrast"] as const;

for (const theme of THEMES) {
  test(`empty workbench · ${theme}`, async ({ page }) => {
    await page.addInitScript((selectedTheme) => {
      localStorage.setItem("sunsetz.theme", selectedTheme);
    }, theme);

    await page.goto("/");
    await expect(page.getByRole("heading", { name: "新会话" })).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "我们今天要一起做些什么？" }),
    ).toBeVisible();

    const geometry = await page.evaluate(() => {
      const composer = document.querySelector(".composer");
      const main = document.querySelector("main.main");
      return {
        horizontalOverflow:
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
        composerWidth: composer?.getBoundingClientRect().width ?? 0,
        mainWidth: main?.getBoundingClientRect().width ?? 0,
      };
    });

    expect(geometry.horizontalOverflow).toBeLessThanOrEqual(0);
    expect(geometry.composerWidth).toBeGreaterThan(300);
    expect(geometry.mainWidth).toBeGreaterThan(360);
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
