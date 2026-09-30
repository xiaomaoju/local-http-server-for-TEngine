import { test, expect, type Page, type WebSocketRoute } from "@playwright/test";
import type { ProjectConfig, VersionEntry } from "../../shared/api";

async function fixture(page: Page, disconnect = false) {
  const user = {
    id: "admin",
    username: "admin",
    role: "admin",
    enabled: true,
    must_change_password: false,
  };
  const project: ProjectConfig = {
    id: "test-project",
    project_name: "交互验证",
    owner_id: "admin",
    can_manage: true,
    shared_with_me: false,
    package_name: "DefaultPackage",
    revision: 1,
    platforms: ["Android"],
    project_versions: ["v1", "v2"].map((name) => ({
      name,
      platform_settings: {
        Android: { access_enabled: true, active_bundle: null },
      },
    })),
  };
  const state = {
    project,
    versions: Array.from({ length: 51 }, (_, i): VersionEntry => ({
      version: `r${51 - i}`,
      file_count: 1,
      total_size: 16,
      modified_timestamp: i,
    })),
    writes: [] as {
      method: string;
      path: string;
      revision?: string;
      body: any;
    }[],
    sockets: [] as WebSocketRoute[],
    failProjects: false,
    failUsers: false,
    auditReads: 0,
  };
  await page.routeWebSocket("**/api/ws/logs?*", async (socket) => {
    state.sockets.push(socket);
    if (disconnect)
      await socket.close({ code: 1011, reason: "proxy unavailable" });
  });
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    const method = req.method();
    const respond = (body: unknown, status = 200) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      });
    if (path === "/api/auth/me") return respond({ user, csrf: "test-csrf" });
    if (path === "/api/auth/ws-ticket")
      return respond({ ticket: "isolated-ticket" });
    if (path === "/api/audit") {
      state.auditReads++;
      return respond([]);
    }
    if (path === "/api/admin/users" && method === "GET")
      return respond(
        state.failUsers ? { message: "用户读取失败" } : [user],
        state.failUsers ? 503 : 200,
      );
    if (path === "/api/projects" && method === "GET")
      return respond(
        state.failProjects ? { message: "读取项目失败" } : [state.project],
        state.failProjects ? 503 : 200,
      );
    if (path === "/api/projects/test-project" && method === "GET")
      return respond(state.project);
    if (path.endsWith("/versions") && method === "GET")
      return respond(state.versions);
    if (method !== "GET") {
      const revision = req.headers()["if-match"];
      const body = req.postDataJSON();
      state.writes.push({ method, path, revision, body });
      if (Number(revision) !== state.project.revision)
        return respond({ message: "项目已被他人修改，请合并最新配置" }, 409);
      if (method === "PUT" && path === "/api/projects/test-project")
        Object.assign(state.project, body);
      if (method === "DELETE")
        state.versions = state.versions.filter(
          (v) => !path.endsWith("/" + v.version),
        );
      state.project.revision++;
      return respond(state.project);
    }
    return respond({ message: `Unhandled test request: ${path}` }, 404);
  });
  await page.goto(
    "/#page=projects&project=test-project&version=v2&platform=Android",
  );
  await expect(
    page.getByRole("heading", { name: "交互验证", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("row")).toHaveCount(51);
  return state;
}

test("socket reconnects preserve rows, modal inputs and pointer/keyboard dismissal safety", async ({
  page,
}) => {
  const state = await fixture(page, true);
  await page.evaluate(() => {
    const row = document.querySelector(".ui-table tbody tr")!;
    (window as any).removedRows = 0;
    new MutationObserver(() => {
      if (!row.isConnected) (window as any).removedRows++;
    }).observe(document.querySelector(".ui-table tbody")!, { childList: true });
  });
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "项目设置", exact: true });
  const input = page.getByLabel("项目名称", { exact: true });
  await input.fill("保留输入");
  await dialog.locator("header").click({ position: { x: 200, y: 20 } });
  await dialog.locator("footer").click({ position: { x: 10, y: 10 } });
  await page.mouse.click(2, 2);
  await expect(dialog).toBeVisible();
  await expect
    .poll(() => state.sockets.length, { timeout: 10000 })
    .toBeGreaterThanOrEqual(3);
  await expect(input).toHaveValue("保留输入");
  await expect(input).toBeEnabled();
  expect(await page.evaluate(() => (window as any).removedRows)).toBe(0);
  await page.keyboard.press("Escape");
  const discard = page.getByRole("dialog", { name: "放弃未保存的修改？" });
  await expect(discard).toBeVisible();
  await discard.getByRole("button", { name: "取消", exact: true }).click();
  await expect(input).toHaveValue("保留输入");
  expect(state.writes).toHaveLength(0);
});

test("disconnected polling keeps the create-project dialog and unsaved input on the project list", async ({
  page,
}) => {
  const state = await fixture(page, true);
  await page.getByRole("button", { name: "返回项目列表", exact: true }).click();
  await page.getByRole("button", { name: "创建项目", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "创建项目", exact: true });
  await page.getByLabel("项目名称", { exact: true }).fill("创建中的项目");
  const previousReads = state.auditReads;
  await expect
    .poll(() => state.auditReads, { timeout: 20000 })
    .toBeGreaterThan(previousReads);
  await expect(dialog).toBeVisible();
  await expect(page.getByLabel("项目名称", { exact: true })).toHaveValue(
    "创建中的项目",
  );
  await expect(page.getByLabel("项目名称", { exact: true })).toBeEnabled();
  expect(state.writes).toHaveLength(0);
});

test("configuration conflicts merge independent fields and require same-field resolution", async ({
  page,
}) => {
  const state = await fixture(page);
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("我的修改");
  state.project.package_name = "RemotePackage";
  state.project.revision++;
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await page.getByRole("button", { name: "合并最新配置" }).click();
  await expect(page.getByLabel("项目名称", { exact: true })).toHaveValue(
    "我的修改",
  );
  await page.getByText("高级设置", { exact: true }).click();
  await expect(page.getByLabel("资源包名", { exact: true })).toHaveValue(
    "RemotePackage",
  );
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "项目设置", exact: true }),
  ).not.toBeVisible();
  expect(state.project.package_name).toBe("RemotePackage");
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("再次修改");
  state.project.project_name = "其他成员修改";
  state.project.revision++;
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await page.getByRole("button", { name: "合并最新配置" }).click();
  await expect(
    page.getByRole("button", { name: "保存修改", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "使用最新值" }).click();
  await expect(page.getByLabel("项目名称", { exact: true })).toHaveValue(
    "其他成员修改",
  );
  await expect(
    page.getByRole("button", { name: "保存修改", exact: true }),
  ).toBeEnabled();
});

test("confirmation keeps its original target and revision across a remote context change", async ({
  page,
}) => {
  const state = await fixture(page);
  await page
    .getByRole("button", { name: "删除资源版本 r51", exact: true })
    .click();
  const confirm = page.getByRole("dialog", { name: "删除资源版本？" });
  await expect(confirm).toContainText("v2");
  state.project.project_versions = state.project.project_versions.filter(
    (v) => v.name !== "v2",
  );
  state.project.revision++;
  state.sockets.at(-1)!.send(JSON.stringify({ type: "access_changed" }));
  await expect(
    page.getByRole("button", {
      name: "v1",
      exact: true,
      includeHidden: true,
    }),
  ).toHaveAttribute("aria-pressed", "true");
  await confirm
    .getByRole("button", { name: "删除资源版本", exact: true })
    .click();
  await expect(confirm.getByRole("alert")).toContainText("项目已被他人修改");
  expect(state.writes.at(-1)?.path).toContain("/project-versions/v2/");
  expect(state.writes.at(-1)?.revision).toBe("1");
  expect(state.versions).toHaveLength(51);
});

test("deleting the last row on page two returns to a populated first page", async ({
  page,
}) => {
  const state = await fixture(page);
  await page.getByRole("button", { name: "下一页", exact: true }).click();
  await page.getByRole("button", { name: "删除资源版本 r1", exact: true }).click();
  await page
    .getByRole("dialog", { name: "删除资源版本？" })
    .getByRole("button", { name: "删除资源版本", exact: true })
    .click();
  await expect(page.getByRole("row")).toHaveCount(51);
  await expect(
    page.getByRole("cell", { name: "r51", exact: true }),
  ).toBeVisible();
  expect(state.versions).toHaveLength(50);
});

test("successful writes report follow-up read failures outside the closed dialog", async ({
  page,
}) => {
  const state = await fixture(page);
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("已经保存");
  state.failProjects = true;
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "项目设置", exact: true }),
  ).not.toBeVisible();
  await expect(page.getByRole("alert")).toContainText(
    "操作已完成，但最新数据读取失败",
  );
  expect(state.project.project_name).toBe("已经保存");
});

test("user forms preserve drafts, lock inputs during writes and clear successful retry errors", async ({
  page,
}) => {
  const state = await fixture(page);
  state.failUsers = true;
  await page.getByRole("button", { name: "用户管理", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("用户读取失败");
  state.failUsers = false;
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await expect(
    page.getByRole("cell", { name: "admin 你", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("alert")).not.toBeVisible();
  await page.getByRole("button", { name: "创建用户", exact: true }).click();
  await page.getByLabel("用户名", { exact: true }).fill("designer");
  await page.getByLabel("初始密码").fill("isolated-password");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "继续编辑", exact: true }).click();
  await expect(page.getByLabel("用户名", { exact: true })).toHaveValue(
    "designer",
  );
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  await page.route("**/api/admin/users", async (route) => {
    await pending;
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ message: "暂时无法保存" }),
    });
  });
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByLabel("用户名", { exact: true })).toBeDisabled();
  await expect(page.getByLabel("初始密码")).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "创建用户", exact: true }),
  ).toBeVisible();
  finish();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "暂时无法保存",
  );
  await expect(page.getByLabel("用户名", { exact: true })).toBeEnabled();
});
