import { defineConfig } from "@playwright/test";
import { resolve } from "node:path";
import { randomUUID } from "node:crypto";
const dataDir = resolve(process.cwd(), "../Cache/tests/browser", randomUUID());
export default defineConfig({
  testDir: "./tests",
  timeout: 45000,
  workers: 1,
  outputDir: "../Cache/tests/browser-results",
  use: {
    baseURL: "http://127.0.0.1:18092",
    viewport: { width: 960, height: 700 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: [
    {
      command: "cargo run --locked --manifest-path ../server/Cargo.toml",
      url: "http://127.0.0.1:18092/api/health",
      timeout: 120000,
      reuseExistingServer: false,
      env: {
        DATA_DIR: dataDir,
        PORT: "18092",
        COOKIE_SECURE: "false",
        BOOTSTRAP_ADMIN_USERNAME: "admin",
        BOOTSTRAP_ADMIN_PASSWORD: "browser-initial-password",
      },
    },
    {
      command:
        "npm --prefix .. run dev -- --host 127.0.0.1 --port 1431 --strictPort",
      url: "http://127.0.0.1:1431",
      reuseExistingServer: false,
    },
  ],
});
