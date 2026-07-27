import { expect, test } from "@playwright/test";

const THEMES = ["dark", "light", "high-contrast"] as const;

for (const theme of THEMES) {
  test(`resource pane lifecycle · ${theme}`, async ({ page }) => {
    await page.addInitScript((selectedTheme) => {
      localStorage.setItem("sunsetz.theme", selectedTheme);
    }, theme);
    await page.goto("/");
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);

    const toggle = page.getByRole("button", { name: "显示文件" });
    await toggle.click();
    const pane = page.locator("aside.aside");
    await expect(pane).toHaveAttribute("aria-hidden", "false");
    await expect(page.getByTestId("resource-viewer")).toBeVisible();

    const geometry = await page.evaluate(() => ({
      horizontalOverflow:
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
      mainWidth:
        document.querySelector("main.main")?.getBoundingClientRect().width ?? 0,
      paneWidth:
        document.querySelector("aside.aside")?.getBoundingClientRect().width ??
        0,
    }));
    expect(geometry.horizontalOverflow).toBeLessThanOrEqual(0);
    expect(geometry.mainWidth).toBeGreaterThan(360);
    expect(geometry.paneWidth).toBeGreaterThan(240);

    await expect(page).toHaveScreenshot(`${theme}-resources-open.png`, {
      animations: "disabled",
      caret: "hide",
      fullPage: false,
      maxDiffPixels: 100,
    });

    await page
      .getByTestId("resource-viewer")
      .getByRole("button", { name: "关闭" })
      .click();
    await expect(pane).toHaveAttribute("aria-hidden", "true");
    await expect(page.getByTestId("resource-viewer")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "显示文件" })).toBeFocused();
  });
}

test("200% zoom keeps the workbench operable", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => {
    document.documentElement.style.zoom = "2";
  });

  await expect(page.getByRole("heading", { name: "新会话" })).toBeVisible();
  await expect(page.getByLabel("隐藏侧栏")).toBeVisible();

  const geometry = await page.evaluate(() => {
    const main = document.querySelector("main.main")?.getBoundingClientRect();
    const composer = document
      .querySelector(".composer")
      ?.getBoundingClientRect();
    return {
      horizontalOverflow:
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
      mainWidth: main?.width ?? 0,
      composerWidth: composer?.width ?? 0,
      composerBottom: composer?.bottom ?? 0,
      viewportHeight: window.innerHeight,
    };
  });

  expect(geometry.horizontalOverflow).toBeLessThanOrEqual(1);
  expect(geometry.mainWidth).toBeGreaterThan(180);
  expect(geometry.composerWidth).toBeGreaterThan(150);
  expect(geometry.composerBottom).toBeLessThanOrEqual(
    geometry.viewportHeight + 1,
  );
});
