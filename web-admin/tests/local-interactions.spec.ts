import { test, expect, type Page } from "@playwright/test";
async function localFixture(page: Page) {
  await page.addInitScript(() => {
    const state = {
      projects: ["A", "B"].map((id, i) => ({
        id,
        project_name: `本地${id}`,
        bundles_dir: "/test/bundles",
        port: 18100 + i,
        platforms: ["Android"],
        cors_enabled: true,
        package_name: "DefaultPackage",
        project_version: "v1",
        platform_access: { Android: true },
      })),
      calls: [] as any[],
      holdList: false,
      holdSave: false,
      releaseList: undefined as undefined | (() => void),
      releaseSave: undefined as undefined | (() => void),
      saved: undefined as any,
    };
    (window as any).localTest = state;
    (window as any).__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      unregisterCallback: () => {},
      invoke: async (cmd: string, args: any) => {
        state.calls.push({
          cmd,
          args: JSON.parse(JSON.stringify(args ?? null)),
        });
        if (cmd === "get_projects") return structuredClone(state.projects);
        if (cmd === "get_running_servers" || cmd === "get_local_ips") return [];
        if (cmd === "get_resource_version")
          return [{ platform: "Android", latest: "r1", synced: null }];
        if (cmd === "list_versions") {
          if (state.holdList)
            await new Promise<void>((resolve) => {
              state.releaseList = resolve;
            });
          return [
            {
              version: args.projectId + "-r1",
              file_count: 1,
              total_size: 1,
              modified_timestamp: 1,
            },
          ];
        }
        if (cmd === "update_project") {
          const saved = JSON.parse(JSON.stringify(args.project));
          if (state.holdSave)
            await new Promise<void>((resolve) => {
              state.releaseSave = resolve;
            });
          state.saved = saved;
          return;
        }
        if (cmd === "start_server") return "http://127.0.0.1:18100/";
        if (cmd === "sync_specific_version")
          return {
            success: true,
            message: "完成",
            version: args.version,
            synced_files: [],
          };
        if (cmd === "plugin:event|listen") return 1;
        if (cmd === "plugin:event|unlisten") return;
        throw new Error("Unexpected test command: " + cmd);
      },
    };
  });
  await page.goto("http://127.0.0.1:1431");
  await expect(
    page.getByRole("button", { name: "本地A", exact: true }),
  ).toBeVisible();
}

test("local sync discards a late version response after switching projects", async ({
  page,
}) => {
  await localFixture(page);
  await page.evaluate(() => {
    (window as any).localTest.holdList = true;
  });
  await page.getByRole("button", { name: "同步资源", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => !!(window as any).localTest.releaseList))
    .toBeTruthy();
  await page.getByRole("button", { name: "本地B", exact: true }).click();
  await page.evaluate(() => {
    const s = (window as any).localTest;
    s.holdList = false;
    s.releaseList();
  });
  await expect(
    page.getByRole("button", { name: "同步资源", exact: true }),
  ).toBeEnabled();
  await expect(page.getByRole("dialog")).not.toBeVisible();
  await page.getByRole("button", { name: "同步资源", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("B-r1");
  await page.getByRole("button", { name: "确认同步", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).localTest.calls.find(
            (c: any) => c.cmd === "sync_specific_version",
          )?.args.projectId,
      ),
    )
    .toBe("B");
});

test("local settings cannot be cancelled while saving and running settings are read-only", async ({
  page,
}) => {
  await localFixture(page);
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("已经保存的名称");
  await page.evaluate(() => {
    (window as any).localTest.holdSave = true;
  });
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(page.getByLabel("项目名称", { exact: true })).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "取消", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "本地项目设置", exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    (window as any).localTest.releaseSave();
  });
  await expect(page.getByRole("dialog")).not.toBeVisible();
  expect(
    await page.evaluate(() => (window as any).localTest.saved.project_name),
  ).toBe("已经保存的名称");
  await page.getByRole("button", { name: "启动服务", exact: true }).click();
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "Android", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("switch", { name: "Android客户端下载", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "保存修改", exact: true }),
  ).toBeDisabled();
});
