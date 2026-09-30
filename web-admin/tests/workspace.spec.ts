import { test, expect, type APIRequestContext } from "@playwright/test";
import { resolve } from "node:path";
const initial = "browser-initial-password";
const password = "browser-changed-password";
async function loginApi(
  request: APIRequestContext,
  username: string,
  pass: string,
) {
  const response = await request.post("/api/auth/login", {
    data: { username, password: pass, client: "desktop" },
  });
  expect(response.ok()).toBeTruthy();
  return (await response.json()).token as string;
}
test("first login, project creation, upload, release, sharing, revocation and compact themes", async ({
  page,
  request,
  browser,
}) => {
  await page.goto("/");
  await page.getByLabel("用户名", { exact: true }).fill("admin");
  await page.getByLabel("密码", { exact: true }).fill("incorrect-password");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(
    page.getByText("用户名或密码错误", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("密码", { exact: true }).fill(initial);
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "首次登录，请修改密码" }),
  ).toBeVisible();
  await page.getByLabel("新密码", { exact: true }).fill(password);
  await page.getByLabel("再次输入新密码").fill(password);
  await page
    .getByRole("button", { name: "修改密码", exact: true })
    .last()
    .click();
  await expect(
    page.getByRole("button", { name: "登录", exact: true }),
  ).toBeVisible();
  await page.getByLabel("密码", { exact: true }).fill(password);
  await page.getByRole("button", { name: "登录", exact: true }).click();
  const createButton = page
    .getByRole("button", { name: "创建项目", exact: true })
    .first();
  await createButton.focus();
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("dialog").getByRole("button", { name: "关闭", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(
    page.getByRole("button", { name: "创建并进入项目" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(createButton).toBeFocused();
  await page.keyboard.press("Enter");
  await page.getByLabel("项目名称", { exact: true }).fill("星海计划");
  await page.getByRole("button", { name: "创建并进入项目" }).click();
  await expect(
    page.getByRole("heading", { name: "星海计划", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "上传资源", exact: true })
    .first()
    .click();
  await page.getByLabel("资源文件", { exact: true }).setInputFiles({
    name: "manifest.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("hello resource"),
  });
  await page.getByLabel("资源版本号（可选）").fill("release-01");
  await page.getByRole("button", { name: "确认上传" }).click();
  await expect(page.getByText("release-01 已上传，尚未发布")).toBeVisible();
  await page.getByRole("button", { name: "发布此版本", exact: true }).click();
  await page.getByRole("button", { name: "确认发布", exact: true }).click();
  await expect(
    page.getByRole("cell", { name: "已发布", exact: true }),
  ).toBeVisible();
  const token = await loginApi(request, "admin", password);
  const auth = { Authorization: `Bearer ${token}` };
  const projects = await (
    await request.get("/api/projects", { headers: auth })
  ).json();
  const id = projects[0].id;
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.getByRole("button", { name: "复制地址", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe(`http://127.0.0.1:18092/res/v1/${encodeURIComponent("星海计划")}/Android/`);
  const download = await request.get(`/res/v1/${encodeURIComponent("星海计划")}/Android/manifest.txt`);
  expect(await download.text()).toBe("hello resource");
  const liveLogs = page.getByRole('region', {name:'客户端日志',exact:true});
  const logToggle = liveLogs.getByRole('button', {name:/^日志/});
  await logToggle.click();
  await liveLogs.getByRole('tab', {name:/资源请求/}).click();
  await expect(liveLogs.getByText(/GET .*manifest.txt · 资源响应读取完成/)).toBeVisible();
  const missing = await request.get(`/res/v1/${encodeURIComponent("星海计划")}/Android/missing.hash`);
  expect(missing.status()).toBe(404);
  await expect(liveLogs.getByText(/GET .*missing.hash/)).toBeVisible();
  await expect(liveLogs.getByTitle('HTTP 404 · 错误')).toBeVisible();
  await page.screenshot({animations:'disabled',path:resolve('../Cache/debug/resource-routing/live-request-logs.png')});
  await logToggle.click();
  for (let i = 2; i <= 12; i++) {
    const r = await request.post(
      `/api/projects/${id}/project-versions/v1/upload`,
      {
        headers: auth,
        multipart: {
          platform: "Android",
          version: `release-${String(i).padStart(2, "0")}`,
          files: {
            name: "manifest.txt",
            mimeType: "text/plain",
            buffer: Buffer.from("sample"),
          },
        },
      },
    );
    expect(r.ok()).toBeTruthy();
  }
  await page.reload();
  await expect(page.getByRole("row")).toHaveCount(13);
  const rows = await page
    .locator(".ui-table tbody tr")
    .evaluateAll((elements) =>
      elements.map((el) => el.getBoundingClientRect().bottom),
    );
  expect(rows.filter((bottom) => bottom < 668).length).toBeGreaterThanOrEqual(
    8,
  );
  await page.screenshot({
    animations: "disabled",
    path: resolve("../Cache/tests/workbench-dark-960.png"),
  });
  await page.getByRole("button", { name: "浅色外观" }).click();
  await page.screenshot({
    animations: "disabled",
    path: resolve("../Cache/tests/workbench-light-960.png"),
  });
  await page.setViewportSize({ width: 800, height: 600 });
  await page.screenshot({
    animations: "disabled",
    path: resolve("../Cache/tests/workbench-light-800.png"),
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBeTruthy();
  await page.getByRole("button", { name: "深色外观" }).click();
  await page.screenshot({
    animations: "disabled",
    path: resolve("../Cache/tests/workbench-dark-800.png"),
  });
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("未保存的修改");
  await page.getByRole("button", { name: "取消", exact: true }).last().click();
  await expect(
    page.getByRole("heading", { name: "放弃未保存的修改？" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await page.getByRole("button", { name: "用户管理", exact: true }).click();
  await page.getByRole("button", { name: "创建用户", exact: true }).click();
  await page.getByLabel("用户名", { exact: true }).fill("designer");
  await page.getByLabel("初始密码", { exact: true }).fill(initial);
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(
    page.getByRole("cell", { name: "designer", exact: true }),
  ).toBeVisible();
  const users = await (
    await request.get("/api/admin/users", { headers: auth })
  ).json();
  const designer = users.find((u: any) => u.username === "designer");
  let designerToken = await loginApi(request, "designer", initial);
  await request.put("/api/auth/password", {
    headers: { Authorization: `Bearer ${designerToken}` },
    data: { current_password: initial, password },
  });
  designerToken = await loginApi(request, "designer", password);
  await page.getByRole("button", { name: "项目", exact: true }).click();
  await page.getByRole("row", { name: "打开项目 星海计划", exact: true }).click();
  await page.getByRole("button", { name: "共享成员", exact: true }).click();
  await page.getByLabel("对方的完整用户名").fill("designer");
  await page.getByRole("button", { name: "添加共享成员", exact: true }).click();
  await expect(
    page
      .getByRole("dialog", { name: "共享成员", exact: true })
      .getByRole("button", { name: "移除", exact: true }),
  ).toBeVisible();
  const context = await browser.newContext({
    viewport: { width: 960, height: 700 },
  });
  const other = await context.newPage();
  await other.goto("http://127.0.0.1:18092/");
  await other.getByLabel("用户名", { exact: true }).fill("designer");
  await other.getByLabel("密码", { exact: true }).fill(password);
  await other.getByRole("button", { name: "登录", exact: true }).click();
  await other.getByRole("radio", { name: "共享给我", exact: true }).click();
  await other.getByRole("row", { name: "打开项目 星海计划", exact: true }).click();
  await expect(
    other.getByRole("heading", { name: "星海计划", exact: true }),
  ).toBeVisible();
  // Keep a write pending past the socket reconnect timer while access is revoked.
  let releaseUpload!: () => void;
  const heldUpload = new Promise<void>((resolve) => {
    releaseUpload = resolve;
  });
  await other.route(
    `**/api/projects/${id}/project-versions/v1/upload`,
    async (route) => {
      await heldUpload;
      await route.continue();
    },
  );
  await other
    .getByRole("button", { name: "上传资源", exact: true })
    .first()
    .click();
  await other.getByLabel("资源文件", { exact: true }).setInputFiles({
    name: "pending.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("pending"),
  });
  await other.getByRole("button", { name: "确认上传", exact: true }).click();
  await page.getByRole("button", { name: "移除", exact: true }).click();
  await page.getByRole("button", { name: "移除成员", exact: true }).click();
  await other.waitForTimeout(2500);
  releaseUpload();
  await expect(
    other.getByRole("heading", { name: "星海计划", exact: true }),
  ).not.toBeVisible({ timeout: 10000 });
  await expect(other.getByRole("alert")).toContainText(
    "项目已被删除，或共享权限已被移除。请联系项目所有者，或选择其他项目。",
  );
  expect(
    (
      await request.get(`/api/projects/${id}`, {
        headers: { Authorization: `Bearer ${designerToken}` },
      })
    ).status(),
  ).toBe(404);
  await context.close();
  await page
    .getByRole("dialog")
    .filter({ has: page.getByRole("heading", { name: "共享成员" }) })
    .getByRole("button", { name: "关闭", exact: true })
    .click();
  await page.getByRole("button", { name: "用户管理", exact: true }).click();
  await page
    .getByRole("row")
    .filter({ has: page.getByText("designer", { exact: true }) })
    .getByRole("button", { name: "删除", exact: true })
    .click();
  await page
    .getByRole("button", { name: "转交并删除账号", exact: true })
    .click();
  await expect(
    page.getByRole("cell", { name: "designer", exact: true }),
  ).not.toBeVisible();
});
