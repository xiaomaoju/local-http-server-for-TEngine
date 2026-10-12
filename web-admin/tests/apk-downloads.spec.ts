import { test, expect, type Page, type Route } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

async function fixture(page: Page, native = false) {
  const project = {
    id: "apk-counts", project_name: "应用下载统计", project_type: "apk", package_name: "", platforms: ["Android"],
    owner_id: "tester", revision: 1, can_manage: true, shared_with_me: false,
    project_versions: ["v1", "v2"].map(name => ({ name, platform_settings: { Android: { access_enabled: true, active_bundle: "70", published_releases: ["68", "69", "70"] } } })),
  };
  const state = { status: 200, total: 128, hold: false, pending: [] as Route[] };
  await page.routeWebSocket("**/api/ws/logs?*", () => {});
  await page.route("**/api/**", async route => {
    const path = new URL(route.request().url()).pathname;
    const send = (body: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/api/auth/me") return send({ user: { id: "tester", username: "tester", role: "admin", enabled: true, must_change_password: false }, csrf: "fixture" });
    if (path === "/api/auth/ws-ticket") return send({ ticket: "fixture" });
    if (path === "/api/audit" || path === "/api/resource-logs") return send([]);
    if (path === "/api/projects") return send([project]);
    if (path.endsWith("/versions")) return send(Array.from({ length: 10 }, (_, i) => ({ version: String(78 - i), file_count: 2, total_size: 2048, modified_timestamp: 1 })));
    if (path.endsWith("/download-stats")) {
      if (state.hold && path.includes("/v1/")) { state.pending.push(route); return; }
      if (state.status !== 200) return send({ message: "统计读取失败" }, state.status);
      return send(path.includes("/v2/") ? { total: 8, releases: [{ release: "70", downloads: 8 }] } : {
        total: state.total, releases: [{ release: "70", downloads: 35 }, { release: "69", downloads: 90 }, { release: "68", downloads: state.total - 125 }],
      });
    }
    return send({}, 404);
  });
  if (native) await page.addInitScript(() => {
    (window as any).nativeFixture = { chooseDirectory: async () => null, versions: async () => [], manifest: async () => [], upload: async () => {}, openUrl: async () => {} };
  });
  const base = native ? "http://127.0.0.1:1431/web-admin/tests/fixtures/native-workspace.html" : "/";
  await page.goto(base + "#page=projects&project=apk-counts&version=v1&platform=Android");
  await expect(page.getByRole("heading", { name: "应用下载统计", exact: true })).toBeVisible();
  await expect(page.getByLabel("APK 下载统计")).toContainText("128 次");
  return state;
}

for (const native of [false, true]) {
  test(`APK per-release counts, history and compact themes (${native ? "desktop workspace" : "web"})`, async ({ page }) => {
    const state = await fixture(page, native);
    await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toHaveText("35");
    await expect(page.getByLabel("资源版本 69 下载次数", { exact: true })).toHaveText("90");
    await expect(page.getByLabel("资源版本 71 下载次数", { exact: true })).toHaveText("0");
    await page.getByRole("button", { name: "统计详情", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "下载统计" });
    await expect(dialog).toContainText("不代表安装成功或独立设备数");
    await expect(dialog.getByRole("row").filter({ hasText: "68" })).toContainText("3");
    await page.keyboard.press("Escape");
    await expect(dialog).not.toBeVisible();
    await expect(page.getByRole("button", { name: "统计详情", exact: true })).toBeFocused();
    state.total = 130;
    await page.getByRole("button", { name: "刷新统计", exact: true }).click();
    await expect(page.getByLabel("APK 下载统计")).toContainText("130 次");
    if (!native) {
      const output = resolve(process.cwd(), "../Cache/tests/apk-downloads");
      await mkdir(output, { recursive: true });
      for (const theme of ["dark", "light"]) {
        if (theme === "light") await page.getByRole("button", { name: "浅色外观" }).click();
        for (const viewport of [{ width: 960, height: 700 }, { width: 800, height: 600 }]) {
          await page.setViewportSize(viewport);
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
          await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toBeVisible();
          await page.screenshot({ path: resolve(output, `${theme}-${viewport.width}.png`), fullPage: true });
        }
      }
    }
  });
}

test("APK statistics distinguish failures and old servers from zero", async ({ page }) => {
  const state = await fixture(page);
  state.status = 500;
  await page.getByRole("button", { name: "刷新统计", exact: true }).click();
  await expect(page.getByLabel("APK 下载统计")).toContainText("暂不可用");
  await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toHaveText("—");
  state.status = 404;
  await page.getByRole("button", { name: "刷新统计", exact: true }).click();
  await expect(page.getByLabel("APK 下载统计")).toContainText("不支持统计");
  state.status = 200;
  await page.getByRole("button", { name: "刷新统计", exact: true }).click();
  await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toHaveText("35");
});

test("late statistics cannot overwrite a newly selected project version", async ({ page }) => {
  const state = await fixture(page);
  state.hold = true;
  await page.getByRole("button", { name: "刷新统计", exact: true }).click();
  await expect.poll(() => state.pending.length).toBeGreaterThan(0);
  await page.getByRole("button", { name: "v2", exact: true }).click();
  await expect(page.getByLabel("APK 下载统计")).toContainText("8 次");
  await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toHaveText("8");
  for (const route of state.pending) await route.fulfill({ contentType: "application/json", body: JSON.stringify({ total: 999, releases: [{ release: "70", downloads: 999 }] }) });
  await expect(page.getByLabel("APK 下载统计")).toContainText("8 次");
  await expect(page.getByLabel("资源版本 70 下载次数", { exact: true })).toHaveText("8");
});
