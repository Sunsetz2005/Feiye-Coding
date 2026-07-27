import { mkdir } from "node:fs/promises";
import path from "node:path";
import { chromium } from "@playwright/test";

const outputDirectory = path.resolve(process.argv[2] ?? ".");
await mkdir(outputDirectory, { recursive: true });

const endpoint = await waitForEndpoint("http://127.0.0.1:9222/json/version");
const browser = await chromium.connectOverCDP(endpoint.webSocketDebuggerUrl);

try {
  const page = await waitForAppPage(browser);
  await enterWorkbench(page);
  await page.locator("html").waitFor({ state: "visible" });

  const initial = await page.evaluate(() => ({
    theme: document.documentElement.dataset.theme,
    horizontalOverflow:
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
    viewport: {
      width: window.innerWidth,
      height: window.innerHeight,
      devicePixelRatio: window.devicePixelRatio,
    },
  }));
  assert(
    ["light", "dark", "high-contrast"].includes(initial.theme),
    `expected a resolved theme, got ${initial.theme}`,
  );
  assert(
    initial.horizontalOverflow <= 0,
    `initial horizontal overflow was ${initial.horizontalOverflow}px`,
  );
  assert(
    initial.viewport.width >= 900 && initial.viewport.height >= 600,
    `unexpected WebView viewport ${initial.viewport.width}x${initial.viewport.height}`,
  );

  const resourceToggle = page.getByRole("button", {
    name: /^(Show files|显示文件)$/,
  });
  await resourceToggle.click();
  await page.getByTestId("resource-viewer").waitFor({ state: "visible" });
  await page.screenshot({
    path: path.join(outputDirectory, "resources-open.png"),
  });

  await page
    .getByTestId("resource-viewer")
    .getByRole("button", { name: /^(Close|关闭)$/ })
    .click();
  await page.getByTestId("resource-viewer").waitFor({ state: "detached" });
  assert(
    await page.getByRole("button", {
      name: /^(Show files|显示文件)$/,
    }).evaluate(
      (element) => element === document.activeElement,
    ),
    "resource toggle did not regain focus",
  );

  await page.getByRole("button", {
    name: /^(Hide sidebar|隐藏侧栏)$/,
  }).click();
  const sidebar = page.getByTestId("sidebar-navigator");
  assert(
    (await sidebar.getAttribute("aria-hidden")) === "true",
    "collapsed sidebar did not expose aria-hidden=true",
  );
  assert(
    await sidebar.evaluate((element) => element.inert),
    "collapsed sidebar was not inert",
  );
  assert(
    await page.getByRole("button", {
      name: /^(Show sidebar|显示侧栏)$/,
    }).evaluate(
      (element) => element === document.activeElement,
    ),
    "sidebar toggle did not regain focus",
  );

  await page.getByRole("button", {
    name: /^(Show sidebar|显示侧栏)$/,
  }).press("Space");
  assert(
    (await sidebar.getAttribute("aria-hidden")) === "false",
    "Space did not reopen the sidebar",
  );

  const finalGeometry = await page.evaluate(() => ({
    horizontalOverflow:
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
    composerVisible:
      (document.querySelector(".composer")?.getBoundingClientRect().bottom ??
        Number.POSITIVE_INFINITY) <= window.innerHeight + 1,
  }));
  assert(
    finalGeometry.horizontalOverflow <= 0,
    `final horizontal overflow was ${finalGeometry.horizontalOverflow}px`,
  );
  assert(finalGeometry.composerVisible, "composer left the native viewport");
} finally {
  await browser.close();
}

async function enterWorkbench(page) {
  const deadline = Date.now() + 30_000;
  const newSession = page.getByRole("button", {
    name: /^(New session|新建会话)$/,
  });
  const setupActions = [
    /^(Already installed — recheck|已安装，重新检测)$/,
    /^(Continue|继续)$/,
    /^(Skip for now|暂时跳过)$/,
    /^(Enter Sunsetz|进入 Sunsetz)$/,
  ];

  while (Date.now() < deadline) {
    if (await newSession.isVisible().catch(() => false)) return;
    for (const name of setupActions) {
      const button = page.getByRole("button", { name });
      if (await button.isVisible().catch(() => false)) {
        await button.click();
        break;
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }

  throw new Error("Native app did not enter the workbench within 30 seconds");
}

async function waitForEndpoint(url) {
  const deadline = Date.now() + 30_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return await response.json();
      lastError = new Error(`CDP endpoint returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`WebView2 CDP endpoint unavailable: ${lastError}`);
}

async function waitForAppPage(browser) {
  const deadline = Date.now() + 30_000;
  let observedUrls = [];
  while (Date.now() < deadline) {
    for (const context of browser.contexts()) {
      const pages = context.pages();
      observedUrls = pages.map((candidate) => candidate.url());
      const page = pages.find((candidate) => {
        const url = candidate.url();
        return (
          url.startsWith("tauri://") ||
          url.startsWith("http://tauri.localhost") ||
          url.startsWith("https://tauri.localhost")
        );
      });
      if (page) return page;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(
    `No Tauri application page appeared in the native WebView; observed: ${
      observedUrls.join(", ") || "(none)"
    }`,
  );
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
