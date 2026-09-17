import { expect, test } from "@playwright/test";

test("settings shell is dense, searchable, and capability honest", async ({
  page,
}) => {
  await page.goto("/tests/visual/fixtures/settings.html");

  const settingsNav = page.getByRole("complementary", { name: "设置" });
  await expect(settingsNav).toBeVisible();
  await expect(
    settingsNav.getByRole("button", { name: "返回应用" }),
  ).toBeVisible();
  await expect(
    settingsNav.getByRole("button", { name: "常规" }),
  ).toHaveAttribute("aria-current", "page");
  for (const section of [
    "常规",
    "外观",
    "账户",
    "我的模型",
    "已归档会话",
    "扩展",
    "运行时",
    "关于",
  ]) {
    await expect(
      settingsNav.getByRole("button", { name: section, exact: true }),
    ).toBeVisible();
  }
  await expect(settingsNav).not.toContainText("电脑控制");
  await expect(settingsNav).not.toContainText("语音");

  const search = page.getByRole("textbox", { name: "搜索设置…" });
  await search.fill("默认权限");
  await expect(
    settingsNav.getByRole("button", { name: "常规" }),
  ).toBeVisible();
  await expect(
    settingsNav.getByRole("button", { name: "外观" }),
  ).toHaveCount(0);

  await search.fill("语音");
  await expect(settingsNav.getByRole("status")).toHaveText(
    "没有匹配的设置",
  );

  await search.fill("");
  const main = page.getByRole("main");
  await expect(
    main.getByRole("heading", { name: "常规", level: 1 }),
  ).toBeVisible();
  await expect(main).toContainText("默认权限");
  await expect(main).toContainText("Sunsetz 4.5");
  await search.evaluate((element) => element.blur());

  const geometry = await page.locator(".settings-page").evaluate((element) => {
    const nav = element.querySelector(".settings-page__nav")!;
    const content = element.querySelector(".settings-page__content")!;
    const navRect = nav.getBoundingClientRect();
    const contentRect = content.getBoundingClientRect();
    return {
      navLeft: navRect.left,
      navRight: navRect.right,
      contentLeft: contentRect.left,
      contentRight: contentRect.right,
      viewportWidth: window.innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
    };
  });
  expect(geometry.navLeft).toBeGreaterThanOrEqual(0);
  expect(geometry.contentLeft).toBeGreaterThanOrEqual(geometry.navRight - 1);
  expect(geometry.contentRight).toBeLessThanOrEqual(geometry.viewportWidth);
  expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.viewportWidth);

  await expect(page).toHaveScreenshot("settings-general.png", {
    animations: "disabled",
    caret: "hide",
    fullPage: false,
    maxDiffPixels: 100,
  });
});
