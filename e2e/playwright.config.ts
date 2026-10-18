import { defineConfig, devices } from "@playwright/test";

// E2E_BASE_URL set: run against a deployed instance (emails read back through Resend).
// Otherwise: start the local backend on a fresh database with the "log" mail driver.
const remote = !!process.env.E2E_REMOTE;
const baseURL = process.env.E2E_BASE_URL ?? "http://localhost:8090";

const chromiumArgs = ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"];

export default defineConfig({
  testDir: "./tests",
  timeout: 180_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    launchOptions: { args: chromiumArgs },
    ...devices["Desktop Chrome"],
  },
  webServer: remote
    ? undefined
    : {
        command: "sh ./scripts/start-local.sh",
        url: `${baseURL}/api/health`,
        reuseExistingServer: false,
        timeout: 60_000,
        stdout: "pipe",
      },
});
