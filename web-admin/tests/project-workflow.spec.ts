import { test, expect, type Page } from "@playwright/test";
import { compareManifests } from "../../shared/uploadDiff";
import type { LogEntry } from "../../shared/api";

const longName = "defaultpackage_assets_assetsraw_arts_commons_materials_".repeat(3) + "distinctive-ending-123456.bundle";
async function fixture(page: Page, native = false) {
  const project = (id: string, name: string) => ({ id, project_name: name, package_name: "DefaultPackage", owner_id: "u1", revision: 1, can_manage: true, shared_with_me: false, platforms: ["Android", "iOS"], project_versions: ["v1", "v2"].map(name => ({ name, platform_settings: { Android: { access_enabled: true, active_bundle: name === "v1" ? "old" : null }, iOS: { access_enabled: true, active_bundle: null } } })) });
  const state = { projects: [project("p1", "星海计划"), project("p2", "协作项目")], uploads: 0, publications: 0, failPublish: false, concurrentPublish: false, versions: [{version: "old", file_count: 3, total_size: 30, modified_timestamp: 1}] };
  await page.routeWebSocket("**/api/ws/logs?*", () => {});
  await page.route("**/api/**", async route => {
    const request = route.request(), url = new URL(request.url()), path = url.pathname, method = request.method();
    const send = (body: unknown, status = 200) => route.fulfill({status, contentType: "application/json", body: JSON.stringify(body)});
    if (path === "/api/auth/me") return send({user: {id: "u1", username: "children", role: "admin", enabled: true, must_change_password: false}, csrf: "fixture-csrf"});
    if (path === "/api/auth/ws-ticket") return send({ticket: "fixture"});
    if (path === "/api/auth/logout") return send({});
    if (path === "/api/audit") return send([]);
    if (path === "/api/resource-logs") return send([]);
    if (path === "/api/projects") {
      if (method === "GET") return send(state.projects);
      const body = request.postDataJSON();
      const created = project("new-project", body.project_name);created.platforms = body.platforms;state.projects.push(created);return send(created);
    }
    const p = state.projects.find(p => path.startsWith('/api/projects/' + p.id))!;
    if (path === '/api/projects/' + p?.id) {
      if (method === 'PUT') {
        const update=request.postDataJSON();Object.assign(p,{project_name:update.project_name,package_name:update.package_name,platforms:update.platforms,revision:p.revision+1});
        for (const v of p.project_versions) for (const plat of p.platforms) (v.platform_settings as any)[plat] ||= {access_enabled:true,active_bundle:null};
      }
      return send(p);
    }
    if (path.endsWith("/project-versions") && method === "POST") {
      const {name} = request.postDataJSON();p.project_versions.push({name, platform_settings: {Android: {access_enabled: true, active_bundle: null}, iOS: {access_enabled: true, active_bundle: null}}});p.revision++;return send(p);
    }
    if (path.endsWith("/versions")) return send(state.versions);
    if (path.endsWith("/files")) return send([{name: longName, size: 10, modified_timestamp: 1}]);
    if (path.endsWith("/manifest")) return send([{name: "same.bundle", size: 10, md5: "same"}, {name: "changed.bundle", size: 8, md5: "old"}, {name: "removed.bundle", size: 12, md5: "removed"}]);
    if (path.endsWith("/upload") && method === "POST") {
      state.uploads++;
      const version = request.headers()["content-type"]?.includes("application/json") ? request.postDataJSON().version : request.postData()!.match(/name="version"\r\n\r\n([^\r]+)/)![1];
      state.versions.unshift({version, file_count: 3, total_size: 25, modified_timestamp: 2});p.revision++;
      if (state.concurrentPublish) p.project_versions[0].platform_settings.Android.active_bundle = "someone-else";
      return send({version, project: p});
    }
    if (path.endsWith("/activate")) {
      state.publications++;
      if (state.failPublish) return send({message: "发布暂时不可用"}, 503);
      const segments = path.split('/');const version = p.project_versions.find(v => v.name === decodeURIComponent(segments[5]))!;
      version.platform_settings[url.searchParams.get('platform') as 'Android' | 'iOS'].active_bundle = decodeURIComponent(segments[7]);p.revision++;return send(p);
    }
    return send({message: path}, 404);
  });
  if (native) await page.addInitScript(() => {
    (window as any).directorySelections = 0;
    (window as any).nativeFixture = {
      chooseDirectory: async () => { (window as any).directorySelections++; return "/test/Bundles"; },
      versions: async () => [{version: "native-r2", file_count: 3, total_size: 25, modified_timestamp: 2}],
      manifest: async () => [{name: "same.bundle", size: 10, md5: "same"}, {name: "changed.bundle", size: 11, md5: "new"}, {name: "added.bundle", size: 4, md5: "added"}],
      upload: async (_directory: string, p: any, pv: string, platform: string, version: string) => { const r = await fetch(`/api/projects/${p.id}/project-versions/${pv}/upload`, {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({platform, version})});if (!r.ok) throw Error("upload failed"); },
      openUrl: async () => {},
    };
  });
  await page.goto(native ? "http://127.0.0.1:1431/web-admin/tests/fixtures/native-workspace.html" : "/");
  await expect(page.getByRole('heading', {name: '项目', exact: true})).toBeVisible();
  return state;
}

test('platform switches, clickable project rows and persistent project/version tabs', async ({page}) => {
  await fixture(page);
  const row = page.getByRole('row', {name: '打开项目 星海计划'});
  await expect(row).toContainText('v1 · v2');
  await expect(page.getByRole('columnheader', {name: '操作', exact: true})).toHaveCount(0);
  await row.getByText('Android · iOS').click();
  await page.getByRole('button', {name:'v2', exact:true}).click();
  await page.getByRole('button', {name:'协作项目 · Bundle', exact:true}).click();
  await page.getByRole('button', {name:'星海计划 · Bundle', exact:true}).click();
  await expect(page.getByRole('button', {name:'v2', exact:true})).toHaveAttribute('aria-pressed','true');
  await page.getByRole('button', {name:'新增版本', exact:true}).click();
  await page.getByLabel('项目版本名称', {exact:true}).fill('v3');
  await page.getByRole('button', {name:'添加版本', exact:true}).click();
  await expect(page.getByRole('button', {name:'v3', exact:true})).toHaveAttribute('aria-pressed','true');
  await page.getByRole('button', {name:'新建项目', exact:true}).click();
  const android=page.getByRole('switch',{name:'Android',exact:true}),ios=page.getByRole('switch',{name:'iOS',exact:true});
  await expect(android).toBeChecked();await expect(ios).not.toBeChecked();await ios.click();await expect(ios).toBeChecked();await android.click();await expect(android).not.toBeChecked();
});

test('release preference is scoped and native directory/diff survive reopening', async ({page}) => {
  await fixture(page,true);
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  await page.getByRole('radio',{name:'自动发布',exact:true}).click();
  await page.getByRole('button',{name:'v2',exact:true}).click();await expect(page.getByRole('radio',{name:'手动发布',exact:true})).toBeChecked();
  await page.getByRole('button',{name:'v1',exact:true}).click();await expect(page.getByRole('radio',{name:'自动发布',exact:true})).toBeChecked();
  await page.getByRole('button',{name:'iOS',exact:true}).click();await expect(page.getByRole('radio',{name:'手动发布',exact:true})).toBeChecked();
  await page.getByRole('button',{name:'Android',exact:true}).click();
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();await page.getByRole('button',{name:'选择 Bundles 目录',exact:true}).click();
  await expect(page.getByText('节省 10 B（40%）',{exact:true})).toBeVisible();await page.getByRole('button',{name:'查看差异',exact:true}).click();
  const diff=page.getByRole('dialog',{name:'上传差异 · native-r2'});await expect(diff).toContainText('已修改 1');await expect(diff).toContainText('新增 1');await expect(diff).toContainText('未变化 1');await expect(diff).toContainText('旧版有，本地已移除 1');
  await diff.getByRole('button',{name:'返回上传'}).click();await page.getByRole('dialog',{name:'上传资源',exact:true}).getByRole('button',{name:'取消',exact:true}).click();await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  await page.reload();await page.getByRole('button',{name:'上传资源',exact:true}).first().click();await expect(page.getByText('/test/Bundles',{exact:true})).toBeVisible();await expect(page.getByLabel('本地资源版本')).toHaveValue('native-r2');expect(await page.evaluate(()=>(window as any).directorySelections)).toBe(0);
  await expect(page.getByRole('button',{name:'上传并自动发布',exact:true})).toBeEnabled();
});

for (const outcome of ['manual','automatic','failure','concurrent']) test(`upload ${outcome} has truthful release status`, async ({page}) => {
  const state=await fixture(page,true);state.failPublish=outcome==='failure';state.concurrentPublish=outcome==='concurrent';
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();if(outcome!=='manual')await page.getByRole('radio',{name:'自动发布',exact:true}).click();
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();await page.getByRole('button',{name:'选择 Bundles 目录',exact:true}).click();
  await page.getByRole('button',{name:outcome==='manual'?'确认上传':'上传并自动发布',exact:true}).click();
  if(outcome==='automatic'){await expect(page.getByText('上传完成并已自动发布，客户端将读取此版本。')).toBeVisible();expect(state.publications).toBe(1);}else{await expect(page.getByText('native-r2 已上传，尚未发布')).toBeVisible();if(outcome!=='manual')await expect(page.getByRole('alert')).toContainText('资源已上传，但自动发布失败');expect(state.publications).toBe(outcome==='failure'?1:0);}
  expect(state.uploads).toBe(1);
});

test('long file names stay complete and deletion is a direct row action', async ({page}) => {
  await fixture(page);await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  await expect(page.getByRole('button',{name:'删除资源版本 old',exact:true})).toBeDisabled();await expect(page.getByRole('button',{name:/更多操作/})).toHaveCount(0);
  await page.getByRole('button',{name:'查看文件',exact:true}).click();const name=page.getByRole('cell',{name:longName,exact:true});await expect(name).toBeVisible();expect(await name.evaluate(el=>getComputedStyle(el).whiteSpace)).toBe('normal');expect(await name.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBeTruthy();
});

test('manifest comparison keeps all four categories and exact byte counts', () => {
  const diff=compareManifests([{name:'same',size:10,md5:'a'},{name:'changed',size:5,md5:'new'},{name:'new',size:3,md5:'n'}],[{name:'same',size:10,md5:'a'},{name:'changed',size:5,md5:'old'},{name:'gone',size:12,md5:'g'}],'base');
  expect([diff.unchanged[0].name,diff.modified[0].name,diff.added[0].name,diff.removed[0].name]).toEqual(['same','changed','new','gone']);expect(diff.totalSize).toBe(18);expect(diff.uploadSize).toBe(8);
});

test('platform tabs add a platform without replacing existing project configuration', async ({page}) => {
  const state=await fixture(page);
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  await expect(page.getByRole('combobox',{name:'平台',exact:true})).toHaveCount(0);
  await page.getByRole('button',{name:'新增平台',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'新增平台'});
  await dialog.getByRole('button',{name:'Windows',exact:true}).click();
  await dialog.getByRole('button',{name:'添加平台',exact:true}).click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole('button',{name:'Windows',exact:true})).toHaveAttribute('aria-pressed','true');
  expect(state.projects[0].platforms).toEqual(['Android','iOS','Windows']);
  expect(state.projects[0].project_versions[0].platform_settings.Android.active_bundle).toBe('old');
  await page.reload();
  await expect(page.getByRole('button',{name:'Windows',exact:true})).toHaveAttribute('aria-pressed','true');
});

test('upload automatically refreshes latest local and uploaded versions while keeping manual comparison',async({page})=>{
  const state=await fixture(page,true);
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  // A collaborator uploaded this after the workbench loaded; it is not published.
  state.versions.unshift({version:'newer-upload',file_count:3,total_size:30,modified_timestamp:3});
  await page.evaluate(()=>{(window as any).nativeFixture.versions=async()=>[
    {version:'native-r1',file_count:3,total_size:25,modified_timestamp:1},
    {version:'native-r10',file_count:3,total_size:25,modified_timestamp:10},
    {version:'native-r2',file_count:3,total_size:25,modified_timestamp:2},
  ];});
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();
  await page.getByRole('button',{name:'选择 Bundles 目录',exact:true}).click();
  await expect(page.getByLabel('本地资源版本')).toHaveValue('native-r10');
  await expect(page.getByLabel('对比基准版本')).toHaveValue('newer-upload');
  await page.getByLabel('对比基准版本').selectOption('old');
  await page.getByLabel('本地资源版本').selectOption('native-r2');
  await expect(page.getByLabel('对比基准版本')).toHaveValue('old');
  await page.getByRole('dialog',{name:'上传资源',exact:true}).getByRole('button',{name:'取消',exact:true}).click();
  await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();
  await expect(page.getByLabel('本地资源版本')).toHaveValue('native-r10');
  await expect(page.getByLabel('对比基准版本')).toHaveValue('newer-upload');
  expect(await page.evaluate(()=>(window as any).directorySelections)).toBe(1);
  await page.getByLabel('本地资源版本').selectOption('native-r2');
  state.versions.unshift({version:'native-r2',file_count:3,total_size:25,modified_timestamp:11});
  await page.getByRole('button',{name:'取消',exact:true}).click();await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();
  await page.getByLabel('本地资源版本').selectOption('native-r2');
  await expect(page.getByRole('button',{name:'确认上传',exact:true})).toBeDisabled();
});

test('exclusive controls expose radio behavior and disabled actions explain themselves',async({page})=>{
  await fixture(page,true);
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  const modes=page.getByRole('radiogroup',{name:'发布方式',exact:true});
  await expect(modes.getByRole('radio',{checked:true})).toHaveCount(1);
  const manual=modes.getByRole('radio',{name:'手动发布',exact:true});
  await manual.focus();await manual.press('ArrowRight');
  await expect(modes.getByRole('radio',{name:'自动发布',exact:true})).toBeChecked();
  await expect(manual).not.toBeChecked();
  const download=page.getByRole('switch',{name:'允许客户端下载',exact:true});
  await expect(download).toBeChecked();
  const blocked=page.getByRole('group',{name:'删除资源版本 old',exact:true});
  await blocked.focus();
  await expect(blocked).toHaveAccessibleDescription('当前版本已发布，请先切换或取消发布。');
  await expect(page.locator('.ui-action-help')).toHaveText('当前版本已发布，请先切换或取消发布。');
  await blocked.press('Escape');await expect(page.locator('.ui-action-help')).toHaveCount(0);
  await page.getByRole('button',{name:'上传资源',exact:true}).first().click();
  const source=page.getByRole('radiogroup',{name:'上传来源',exact:true});
  await expect(source.getByRole('radio',{name:'本地构建目录',exact:true})).toBeChecked();
  await source.getByRole('radio',{name:'文件上传',exact:true}).check();
  await expect(source.getByRole('radio',{name:'本地构建目录',exact:true})).not.toBeChecked();
  await source.getByRole('radio',{name:'本地构建目录',exact:true}).check();
  await page.getByRole('button',{name:'选择 Bundles 目录',exact:true}).click();
  await page.getByRole('button',{name:'重新读取',exact:true}).click();
  await expect(page.getByLabel('本地资源版本')).toHaveValue('native-r2');
  await expect(page.getByRole('button',{name:'上传并自动发布',exact:true})).toBeEnabled();
  for(const theme of ['dark','light']) {
    await page.evaluate(t=>{document.documentElement.dataset.theme=t;document.documentElement.style.colorScheme=t},theme);
    for(const width of [960,800]) {
      await page.setViewportSize({width,height:width===960?700:600});
      await page.screenshot({animations:'disabled',path:`../Cache/debug/ui-affordance/upload-${theme}-${width}.png`});
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
    }
  }
});

test('desktop log drawer scopes live events, retains upload errors and clears revoked history', async ({page}) => {
  const state = await fixture(page, true);
  await page.getByRole('button', {name:'星海计划 · Bundle', exact:true}).click();
  const drawer = page.getByRole('region', {name:'客户端日志', exact:true});
  const toggle = drawer.getByRole('button', {name:/^日志/});
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await toggle.click();
  await expect(drawer.getByText('操作记录与客户端事件，独立保留最近 500 条。')).toBeVisible();
  const event = (id:number, projectId:string, message:string): LogEntry => ({id, project_id:projectId, message, actor_id:'u1', actor_name:'children', action:'publish', object:'v1/Android', timestamp:new Date().toISOString()});
  await page.evaluate(log => (window as any).emitLog(log), event(10,'p1','星海发布记录'));
  await page.evaluate(log => (window as any).emitLog(log), event(11,'p2','协作发布记录'));
  await expect(drawer.getByText(/星海发布记录/)).toBeVisible();
  await expect(drawer.getByText(/协作发布记录/)).toHaveCount(0);
  await drawer.getByRole('button', {name:'清空视图', exact:true}).click();
  await expect(drawer.getByText(/星海发布记录/)).toHaveCount(0);
  await page.evaluate(log => (window as any).emitLog(log), event(12,'p1','清空后的新记录'));
  await expect(drawer.getByText(/清空后的新记录/)).toBeVisible();
  state.failPublish = true;
  await page.getByRole('radio', {name:'自动发布',exact:true}).check();
  await page.getByRole('button', {name:'上传资源',exact:true}).click();
  await page.getByRole('button', {name:'选择 Bundles 目录',exact:true}).click();
  await page.getByRole('button', {name:'上传并自动发布',exact:true}).click();
  await expect(drawer.getByText(/资源已上传，但自动发布失败/)).toBeVisible();
  await expect(toggle).toContainText('1 条错误');
  await page.screenshot({animations:'disabled',path:'../Cache/debug/client-logs/drawer-events-960.png'});
  await page.getByRole('button', {name:'协作项目 · Bundle',exact:true}).click();
  await expect(drawer.getByText(/资源已上传，但自动发布失败/)).toHaveCount(0);
  await expect(drawer.getByText(/协作发布记录/)).toBeVisible();
  state.projects.splice(0,1);
  await page.evaluate(() => (window as any).emitLog({type:'access_changed'}));
  await expect(page.getByRole('button', {name:'星海计划 · Bundle',exact:true})).toHaveCount(0);
  await page.getByRole('button', {name:'项目列表',exact:true}).click();
  await expect(drawer.getByText(/清空后的新记录|自动发布失败/)).toHaveCount(0);
  await page.setViewportSize({width:800,height:600});
  await page.screenshot({animations:'disabled',path:'../Cache/debug/client-logs/drawer-800.png'});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
  await page.getByRole('button', {name:'退出登录',exact:true}).click();
  await expect(drawer).toHaveCount(0);
});

test('resource request flood cannot evict, scroll, count errors in or clear operation logs', async ({page}) => {
  await fixture(page,true);
  await page.getByRole('button',{name:'星海计划 · Bundle',exact:true}).click();
  const drawer=page.getByRole('region',{name:'客户端日志',exact:true});
  await drawer.getByRole('button',{name:/^日志/}).click();
  const operations=drawer.getByRole('tab',{name:/操作日志/});
  const resources=drawer.getByRole('tab',{name:/资源请求/});
  await page.evaluate(()=>{for(let i=0;i<40;i++) (window as any).emitLog({id:100+i,project_id:'p1',message:'保留操作 '+i,actor_id:'u1',actor_name:'children',action:'update',object:'v1',timestamp:new Date().toISOString()});});
  await expect(drawer.getByText(/保留操作 39 ·/)).toHaveCount(1);
  await drawer.getByRole('checkbox',{name:'跟随最新'}).uncheck();
  await drawer.getByRole('tabpanel').evaluate(el=>{el.scrollTop=0;});
  const countBefore=await operations.innerText();
  await page.evaluate(()=>{for(let i=0;i<600;i++) (window as any).emitLog({type:'resource_request',id:'request-'+i,project_id:'p1',method:'GET',path:'/res/v1/星海计划/Android/missing-'+i+'.hash',status:404,outcome:'failed',message:'文件不存在',timestamp:new Date().toISOString()});});
  await expect(resources).toContainText('500');
  await expect(operations).toHaveText(countBefore);
  await expect(drawer.getByText(/保留操作 0 ·/)).toHaveCount(1);
  await expect(drawer.getByText(/missing-599/)).toHaveCount(0);
  await expect(drawer.getByRole('button',{name:/^日志/})).not.toContainText('错误');
  expect(await drawer.getByRole('tabpanel').evaluate(el=>el.scrollTop)).toBe(0);
  await operations.focus();await operations.press('ArrowRight');
  await expect(resources).toBeFocused();
  await expect(resources).toHaveAttribute('aria-selected','true');
  await expect(drawer.getByText(/missing-599/)).toHaveCount(1);
  await expect(drawer.getByText(/保留操作/)).toHaveCount(0);
  await expect(drawer.getByRole('checkbox',{name:'跟随最新'})).toBeChecked();
  await drawer.getByRole('button',{name:'清空视图'}).click();
  await expect(resources).toContainText('0');
  await expect(operations).toHaveText(countBefore);
  await operations.click();
  await expect(drawer.getByRole('checkbox',{name:'跟随最新'})).not.toBeChecked();
  await expect(drawer.getByText(/保留操作 0 ·/)).toHaveCount(1);
  await page.evaluate(()=>(window as any).emitLog({type:'resource_request',id:'fresh',project_id:'p1',method:'GET',path:'/res/v1/星海计划/Android/new.hash',status:200,outcome:'completed',message:'资源响应读取完成',timestamp:new Date().toISOString()}));
  await drawer.getByRole('button',{name:'清空视图'}).click();
  await expect(operations).toContainText('0');
  await expect(resources).toContainText('1');
  await resources.click();
  await expect(drawer.getByText(/new.hash/)).toHaveCount(1);
  await page.setViewportSize({width:800,height:600});
  await page.screenshot({animations:'disabled',path:'../Cache/debug/resource-routing/separate-resource-logs.png'});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
});
