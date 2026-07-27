import { defineConfig, devices } from "@playwright/test";

const baseURL = "http://127.0.0.1:1421";

export default defineConfig({
  testDir: "./tests/visual",
  outputDir: "./test-results/playwright",
  snapshotDir: "./tests/visual/__screenshots__",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "line",
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    locale: "zh-CN",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "compact-900x600",
      use: { viewport: { width: 900, height: 600 } },
    },
    {
      name: "standard-1200x800",
      use: { viewport: { width: 1200, height: 800 } },
    },
    {
      name: "wide-1600x1000",
      use: { viewport: { width: 1600, height: 1000 } },
    },
  ],
  webServer: {
    command: "pnpm dev:ui -- --host 127.0.0.1",
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
