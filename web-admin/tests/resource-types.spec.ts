import { test, expect, type Page } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { publicResourcePath } from "../../shared/api";

async function fixture(page: Page, native = false, directory = false) {
  const projects: any[] = [];
  const releases: Record<string, {version: string; files: string[]}[]> = {};
  const attachments: Record<string, {name: string; size: number; modified_timestamp: number}[]> = {};
  await page.routeWebSocket("**/api/ws/logs?*", () => {});
  await page.route("**/api/**", async route => {
    const request = route.request(), path = new URL(request.url()).pathname, method = request.method();
    const send = (body: unknown, status = 200) => route.fulfill({status, contentType: "application/json", body: JSON.stringify(body)});
    if (path === "/api/auth/me") return send({user:{id:"u1",username:"tester",role:"admin",enabled:true,must_change_password:false},csrf:"fixture"});
    if (path === "/api/auth/ws-ticket") return send({ticket:"fixture"});
    if (path === "/api/audit" || path === "/api/resource-logs") return send([]);
    if (path === "/api/projects") {
      if (method === "GET") return send(projects);
      const body = request.postDataJSON();
      if (projects.some(p => p.project_name === body.project_name && p.project_type === body.project_type)) return send({message:"此类型已存在同名项目"},409);
      const platform = body.project_type === "files" ? "Any" : "Android";
      const project = {...body,id:"p"+projects.length,owner_id:"u1",revision:1,can_manage:true,shared_with_me:false,project_versions:[{name:"v1",platform_settings:{[platform]:{access_enabled:true,active_bundle:null,published_releases:[]}}}]};
      projects.push(project); releases[project.id] = [];
      return send(project);
    }
    const project = projects.find(p => path.startsWith(`/api/projects/${p.id}`));
    if (!project) return send({},404);
    if (path === `/api/projects/${project.id}`) return send(project);
    if (path.includes("/attachments")) {
      const files = attachments[project.id] ||= [];
      if (method !== "GET") {
        if (Number(request.headers()["if-match"]) !== project.revision) return send({message:"项目已被修改，请重新读取后重试"},409);
        if (method === "DELETE") attachments[project.id] = files.filter(file => file.name !== decodeURIComponent(path.split('/').at(-1)!));
        else {
          const names: string[] = request.headers()["content-type"].includes("application/json") ? request.postDataJSON().files : [...request.postData()!.matchAll(/filename="([^"]+)"/g)].map(match => match[1]);
          attachments[project.id] = [...files.filter(file => !names.includes(file.name)), ...names.map(name => ({name,size:16,modified_timestamp:1}))];
        }
        project.revision++;
      }
      return send({files:attachments[project.id],snapshot:String(project.revision),project});
    }
    if (path.endsWith("/versions")) return send(releases[project.id].map(v => ({version:v.version,file_count:v.files.length,total_size:32,modified_timestamp:1})));
    if (path.endsWith("/files")) return send(releases[project.id].find(v => v.version === new URL(request.url()).searchParams.get("version"))?.files.map(name => ({name,size:16,modified_timestamp:1})) || []);
    if (path.endsWith("/upload")) {
      const data = request.headers()["content-type"].includes("application/json") ? request.postDataJSON() : null;
      const version = data?.version || request.postData()!.match(/name="version"\r\n\r\n([^\r]+)/)![1];
      const files = data?.files || [...request.postData()!.matchAll(/filename="([^"]+)"/g)].map(match => match[1]);
      releases[project.id].unshift({version,files}); project.revision++;
      return send({version,project});
    }
    if (path.endsWith("/activate")) {
      const settings = project.project_versions[0].platform_settings[project.platforms[0]];
      const version = path.split("/").at(-2)!;
      settings.active_bundle = request.postDataJSON().unpublish ? null : version;
      if (project.project_type === "apk" && !settings.published_releases.includes(version)) settings.published_releases.push(version);
      project.revision++;
      return send(project);
    }
    return send({},404);
  });
  if (native) await page.addInitScript(enableDirectory => {
    (window as any).nativeFixture = {
      chooseDirectory: async () => { throw new Error("APK must not scan Bundles"); },
      versions: async () => { throw new Error("APK must not scan Bundle versions"); },
      manifest: async () => [], upload: async () => {}, openUrl: async () => {},
      chooseFiles: async (kind: string) => [{name:kind === "files" ? "tutorial.html" : kind === "apk" ? "flip.apk" : "catalog.json",path:"/selected/"+kind,size:16}],
      uploadAttachments: async (files: any[], project: any, projectVersion: string, revision: number) => {
        (window as any).lastAttachmentUpload = {files,projectVersion,revision};
        const response = await fetch(`/api/projects/${project.id}/project-versions/${projectVersion}/attachments`,{method:"POST",headers:{"Content-Type":"application/json","If-Match":String(revision)},body:JSON.stringify({files:files.map(file=>file.name)})});
        return response.json();
      },
      uploadFiles: async (files: any[], project: any, projectVersion: string, version: string) => {
        (window as any).lastNativeUpload = {files,projectType:project.project_type,version};
        const response = await fetch(`/api/projects/${project.id}/project-versions/${projectVersion}/upload`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({version,files:files.map(file=>file.name)})});
        return response.json();
      },
    };
    if (enableDirectory) {
      (window as any).nativeAttachmentFiles = [{path:'/chosen/tutorials/guide.html',name:'guide.html',size:16}];
      (window as any).nativeFixture.readAttachmentDirectory = async (directory: string) => { (window as any).lastAttachmentDirectory = directory; return structuredClone((window as any).nativeAttachmentFiles); };
      const release = (code: string) => ({version:code,version_name:'1.'+code,package_name:'example.app',catalog_path:'/chosen/output/'+code+'/catalog.json',catalog_sha256:'fingerprint-'+code,files:[{name:'app-'+code+'.apk',path:'/chosen/output/'+code+'/app-'+code+'.apk',size:32},{name:'catalog.json',path:'/chosen/output/'+code+'/catalog.json',size:16}]});
      (window as any).nativeApkCatalogs = [release('70'),release('9')];
      (window as any).chooseDirectoryCount = 0;
      (window as any).nativeFixture.chooseDirectory = async () => { (window as any).chooseDirectoryCount++; return '/chosen/任意名称与空格 output'; };
      (window as any).nativeFixture.apkReleases = async () => structuredClone((window as any).nativeApkCatalogs);
      (window as any).nativeFixture.uploadApkRelease = async (directory: string, selected: any, project: any, projectVersion: string) => {
        (window as any).lastApkDirectoryUpload = {directory,version:selected.version,fingerprint:selected.catalog_sha256};
        return (window as any).nativeFixture.uploadFiles(selected.files, project, projectVersion, selected.version);
      };
    }
  }, directory);
  await page.goto(native ? "http://127.0.0.1:1431/web-admin/tests/fixtures/native-workspace.html" : "/");
  await expect(page.getByRole("heading",{name:"项目",exact:true})).toBeVisible();
  return {projects,releases};
}
async function create(page: Page, kind: string, name = "RollCall") {
  await page.getByRole("button",{name:"创建项目",exact:true}).first().click();
  await page.getByRole("radio",{name:kind,exact:true}).check();
  await page.getByLabel("项目名称",{exact:true}).fill(name);
  await page.getByRole("button",{name:"创建并进入项目",exact:true}).click();
}
test("same-name typed projects hide platform and package fields and keep namespace paths", async ({page}) => {
  const state = await fixture(page);
  for (const kind of ["Bundle","APP更新","文件"]) {
    if (state.projects.length) await page.getByRole("button",{name:"项目列表",exact:true}).click();
    await create(page,kind);
    const tab = page.getByRole("button",{name:`RollCall · ${kind}`,exact:true});
    await expect(tab).toHaveText(new RegExp(`RollCall\\s*${{Bundle:"Ⓑ",APP更新:"Ⓐ",文件:"Ⓕ"}[kind]}`));
    await expect(tab).toHaveAttribute("title",`RollCall · ${kind}`);
    if (kind !== "Bundle") await expect(page.getByLabel("平台标签",{exact:true})).toHaveCount(0);
  }
  expect(state.projects.map(p=>p.project_type)).toEqual(["bundle","apk","files"]);
  expect(state.projects[2].platforms).toEqual(["Any"]);
  await page.getByRole("button",{name:"项目列表",exact:true}).click();
  await expect(page.getByRole("row",{name:"打开项目 RollCall"})).toHaveCount(3);
  await page.getByRole("button",{name:"RollCall · APP更新",exact:true}).click();
  await page.getByRole("button",{name:"项目设置",exact:true}).click();
  await expect(page.getByLabel("资源包名",{exact:true})).toHaveCount(0);
  await expect(page.getByText("APP更新 · 创建后不可修改",{exact:true})).toBeVisible();
});
test("APK upload uses a pair and historical APK address remains available", async ({page}) => {
  await fixture(page);
  await create(page,"APP更新","ZFlip5");
  for (const version of ["69","70"]) {
    await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
    await page.getByLabel("APK 安装包",{exact:true}).setInputFiles({name:"flip.apk",mimeType:"application/octet-stream",buffer:Buffer.from("apk")});
    await page.getByLabel("catalog.json",{exact:true}).setInputFiles({name:"catalog.json",mimeType:"application/json",buffer:Buffer.from("{}")});
    await page.getByLabel("发布编号（versionCode）",{exact:true}).fill(version);
    await page.getByRole("button",{name:"确认上传",exact:true}).click();
    await expect(page.getByText(`${version} 已上传，尚未发布`,{exact:true})).toBeVisible();
    await page.getByRole("button",{name:"发布此版本",exact:true}).click();
    await page.getByRole("button",{name:"确认发布",exact:true}).click();
  }
  await expect(page.getByRole("button",{name:"复制 catalog 地址",exact:true})).toHaveAttribute("title","http://127.0.0.1:18092/appversion/v1/ZFlip5/catalog.json");
  await expect(page.getByRole("row").filter({has:page.getByRole("cell",{name:"69",exact:true})}).getByText("历史发布",{exact:true})).toBeVisible();
  await page.getByRole("row").filter({has:page.getByRole("cell",{name:"69",exact:true})}).getByRole("button",{name:"查看文件",exact:true}).click();
  const apkRow = page.getByRole("row").filter({has:page.getByRole("cell",{name:"flip.apk",exact:true})});
  await expect(apkRow.getByRole("button",{name:"复制文件地址",exact:true})).toBeEnabled();
  await page.context().grantPermissions(["clipboard-read","clipboard-write"]);
  await apkRow.getByRole("button",{name:"复制文件地址",exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>navigator.clipboard.readText())).toBe("http://127.0.0.1:18092/appversion/v1/ZFlip5/releases/69/flip.apk");
  await expect(page.getByRole("row").filter({has:page.getByRole("cell",{name:"catalog.json",exact:true})}).getByRole("button",{name:"复制文件地址",exact:true})).toBeDisabled();
});
test("generic upload and APK forms retain compact dark and light layouts", async ({page}) => {
  await fixture(page);
  await create(page,"文件");
  await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
  await page.getByLabel("下载文件",{exact:true}).setInputFiles([{name:"说明",mimeType:"application/octet-stream",buffer:Buffer.from("text")},{name:"empty",mimeType:"application/octet-stream",buffer:Buffer.alloc(0)}]);
  await page.getByLabel("文件版本号（可选）",{exact:true}).fill("files-a");
  await page.getByRole("button",{name:"确认上传",exact:true}).click();
  await page.getByRole("button",{name:"发布此版本",exact:true}).click();
  await page.getByRole("button",{name:"确认发布",exact:true}).click();
  await expect(page.getByRole("button",{name:"复制地址",exact:true})).toHaveAttribute("title","http://127.0.0.1:18092/file/v1/RollCall/");
  await page.getByRole("button",{name:"项目列表",exact:true}).click();
  await create(page,"APP更新");
  await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
  await expect(page.getByRole("button",{name:"确认上传",exact:true})).toBeEnabled();
  const output = resolve("../Cache/debug/resource-types");
  await mkdir(output,{recursive:true});
  for (const theme of ["dark","light"]) for (const width of [960,800]) {
    await page.evaluate(value=>{document.documentElement.dataset.theme=value;document.documentElement.style.colorScheme=value},theme);
    await page.setViewportSize({width,height:width===960?700:600});
    await page.screenshot({animations:"disabled",path:resolve(output,`apk-${theme}-${width}.png`)});
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
  }
});
test("desktop APK upload selects explicit files without Bundle scanning", async ({page}) => {
  await fixture(page,true);
  await create(page,"APP更新");
  await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
  await page.getByRole("button",{name:"选择 APK",exact:true}).click();
  await page.getByRole("button",{name:"选择 catalog.json",exact:true}).click();
  await page.getByLabel("发布编号（versionCode）",{exact:true}).fill("69");
  await page.getByRole("button",{name:"确认上传",exact:true}).click();
  await expect(page.getByText("69 已上传，尚未发布",{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).lastNativeUpload)).toMatchObject({projectType:"apk",version:"69",files:[{name:"flip.apk"},{name:"catalog.json"}]});
});
test("public path builder encodes each component and separates APK releases", () => {
  expect(publicResourcePath({project_name:"中文 项目",project_type:"files"},"v1","Any","说明.pdf")).toBe("/file/v1/%E4%B8%AD%E6%96%87%20%E9%A1%B9%E7%9B%AE/%E8%AF%B4%E6%98%8E.pdf");
  expect(publicResourcePath({project_name:"RollCall",project_type:"apk"},"v1","Android","flip.apk","69")).toBe("/appversion/v1/RollCall/releases/69/flip.apk");
});

test("APK directory restores its arbitrary root and one-click rescans and publishes the latest", async ({page}) => {
  await fixture(page,true,true);
  await create(page,"APP更新");
  await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
  await page.getByRole("button",{name:"选择 APK 构建目录",exact:true}).click();
  await expect(page.getByLabel("本地 APK 版本",{exact:true})).toHaveValue("70");
  await expect(page.getByText('/chosen/任意名称与空格 output',{exact:true})).toBeVisible();
  await page.getByRole("button",{name:"确认上传",exact:true}).click();
  await expect(page.getByText("70 已上传，尚未发布",{exact:true})).toBeVisible();
  await page.reload();
  await page.getByRole("button",{name:"RollCall · APP更新",exact:true}).click();
  await expect(page.getByRole("button",{name:"上传最新版本",exact:true})).toBeVisible();
  await page.getByRole("button",{name:"上传最新版本",exact:true}).click();
  await expect(page.locator("#upload-form").getByText("最新 APK 编号已上传或曾发布，请查看现有版本，或构建新编号后重新读取。",{exact:true})).toBeVisible();
  await expect(page.getByRole("button",{name:"确认上传",exact:true})).toBeDisabled();
  await page.getByRole("button",{name:"取消",exact:true}).click();
  await page.evaluate(() => {
    const latest = structuredClone((window as any).nativeApkCatalogs[0]);
    latest.version = '71'; latest.version_name = '1.71'; latest.catalog_sha256 = 'fingerprint-71';
    (window as any).nativeApkCatalogs.unshift(latest);
  });
  await page.getByRole("radio",{name:"自动发布",exact:true}).check();
  await page.getByRole("button",{name:"上传并发布最新版本",exact:true}).click();
  await expect(page.getByText("上传完成并已自动发布，客户端将读取此版本。",{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).lastApkDirectoryUpload)).toMatchObject({directory:'/chosen/任意名称与空格 output',version:'71',fingerprint:'fingerprint-71'});
  expect(await page.evaluate(()=>(window as any).chooseDirectoryCount)).toBe(0);
});
test("APK directory errors block upload and preserve compact layouts in both themes", async ({page}) => {
  await fixture(page,true,true);
  await create(page,"APP更新");
  await page.getByRole("button",{name:"上传资源",exact:true}).first().click();
  await page.getByRole("button",{name:"选择 APK 构建目录",exact:true}).click();
  const output=resolve('../Cache/debug/apk-directory'); await mkdir(output,{recursive:true});
  for (const theme of ['dark','light']) for (const width of [960,800]) {
    await page.evaluate(value=>{document.documentElement.dataset.theme=value;document.documentElement.style.colorScheme=value},theme);
    await page.setViewportSize({width,height:width===960?700:600});
    await page.screenshot({animations:'disabled',path:resolve(output,`directory-${theme}-${width}.png`)});
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
  }
  await page.evaluate(()=>{(window as any).nativeFixture.apkReleases=async()=>{throw new Error('最新编号目录缺少 catalog.json')};});
  await page.getByRole("button",{name:"重新读取",exact:true}).click();
  await expect(page.locator('#upload-form').getByText('最新编号目录缺少 catalog.json',{exact:true})).toBeVisible();
  await expect(page.getByRole("button",{name:"确认上传",exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as any).lastNativeUpload)).toBeUndefined();
});

test("tutorial attachments preserve drafts, show overwrite decisions, and update independently", async ({page}) => {
  const state = await fixture(page);
  await create(page,"APP更新","Tutorial");
  await page.getByRole("button",{name:"教程附件",exact:true}).click();
  const dialog = page.getByRole("dialog",{name:"教程附件",exact:true});
  const input = dialog.getByLabel("选择教程附件");
  await expect(input).toBeEnabled();
  await input.setInputFiles([{name:"tutorial.html",mimeType:"text/html",buffer:Buffer.from("first")},{name:"style.css",mimeType:"text/css",buffer:Buffer.from("body{}") }]);
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByText("教程附件已更新并立即生效。",{exact:true})).toBeVisible();
  expect(state.releases[state.projects[0].id]).toEqual([]);
  expect(state.projects[0].project_versions[0].platform_settings.Android.active_bundle).toBeNull();
  const row = dialog.getByRole("row").filter({has:page.getByRole("cell",{name:"tutorial.html",exact:true})});
  await expect(row.getByRole("button",{name:"打开网页"})).toHaveAttribute("title","http://127.0.0.1:18092/appversion/v1/Tutorial/docs/tutorial.html");
  await expect(row.getByRole("button",{name:"打开网页"})).toBeEnabled();
  await input.setInputFiles({name:"tutorial.html",mimeType:"text/html",buffer:Buffer.from("second")});
  await expect(dialog.getByText("将覆盖：tutorial.html",{exact:true})).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("dialog",{name:"放弃未上传的附件？"}).getByRole("button",{name:"继续编辑"}).click();
  state.projects[0].revision++;
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByRole("alert")).toContainText("项目已被修改");
  await expect(dialog.getByText("将覆盖：tutorial.html",{exact:true})).toBeVisible();
  await dialog.getByRole("button",{name:"重新读取"}).click();
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByText("教程附件已更新并立即生效。",{exact:true})).toBeVisible();
  await expect(dialog.getByRole("cell",{name:"style.css",exact:true})).toBeVisible();
  const output = resolve("../Cache/tests/tutorial-attachments/ui"); await mkdir(output,{recursive:true});
  for (const theme of ["dark","light"]) for (const width of [960,800]) {
    await page.evaluate(value=>{document.documentElement.dataset.theme=value;document.documentElement.style.colorScheme=value},theme);
    await page.setViewportSize({width,height:width===960?700:600});
    await page.screenshot({animations:"disabled",path:resolve(output,`attachments-${theme}-${width}.png`)});
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
  }
  await row.getByRole("button",{name:"删除",exact:true}).click();
  await page.getByRole("dialog",{name:"删除教程附件？"}).getByRole("button",{name:"确认删除"}).click();
  await expect(dialog.getByRole("cell",{name:"tutorial.html",exact:true})).toHaveCount(0);
  await expect(dialog.getByRole("cell",{name:"style.css",exact:true})).toBeVisible();
});

test("desktop remote attachments use native selection and upload with the displayed revision", async ({page}) => {
  await fixture(page,true);
  await create(page,"APP更新");
  await page.getByRole("button",{name:"教程附件",exact:true}).click();
  const dialog = page.getByRole("dialog",{name:"教程附件",exact:true});
  await dialog.getByRole("button",{name:"选择附件",exact:true}).click();
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByRole("cell",{name:"tutorial.html",exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).lastAttachmentUpload)).toMatchObject({projectVersion:"v1",revision:1,files:[{name:"tutorial.html",path:"/selected/files"}]});
});

test("attachment directories are remembered per project and refreshed before uploading", async ({page}) => {
  await fixture(page,true,true);
  await create(page,"APP更新","GenericTutorial");
  const trigger = page.getByRole("button",{name:"教程附件",exact:true});
  await trigger.click();
  const dialog = page.getByRole("dialog",{name:"教程附件",exact:true});
  await dialog.getByRole("button",{name:"选择附件目录",exact:true}).click();
  await expect(dialog.getByText('/chosen/任意名称与空格 output',{exact:true})).toBeVisible();
  await page.evaluate(()=>(window as any).nativeAttachmentFiles.push({path:'/chosen/tutorials/image.png',name:'image.png',size:10}));
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByText("目录文件列表已变化，请检查后再次确认上传。",{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).lastAttachmentUpload)).toBeUndefined();
  await dialog.getByRole("button",{name:"确认上传并生效"}).click();
  await expect(dialog.getByRole("cell",{name:"guide.html",exact:true})).toBeVisible();
  await expect(dialog.getByRole("cell",{name:"image.png",exact:true})).toBeVisible();
  await dialog.getByRole("button",{name:"关闭",exact:true}).last().click();
  await page.reload();
  await trigger.click();
  await expect(dialog.getByText('/chosen/任意名称与空格 output',{exact:true})).toBeVisible();
  await expect(dialog.getByText(/待上传：guide.html/)).toBeVisible();
  expect(await page.evaluate(()=>(window as any).chooseDirectoryCount)).toBe(0);
  const output=resolve('../Cache/tests/tutorial-attachments/ui');await mkdir(output,{recursive:true});
  await page.screenshot({animations:'disabled',path:resolve(output,'attachments-remembered-directory.png')});
  await dialog.getByRole("button",{name:"关闭",exact:true}).last().click();
  await page.getByRole("button",{name:"放弃并关闭",exact:true}).click();
  await page.getByRole("button",{name:"项目列表",exact:true}).click();
  await create(page,"APP更新","AnotherTutorial");
  await trigger.click();
  await expect(dialog.getByRole("button",{name:"选择附件目录",exact:true})).toBeVisible();
  await expect(dialog.getByText('/chosen/任意名称与空格 output',{exact:true})).toHaveCount(0);
});

test("browser directory upload accepts arbitrary flat HTML and rejects nested directories", async ({page}) => {
  await fixture(page); await create(page,"APP更新","FlatDocs");
  await page.getByRole("button",{name:"教程附件",exact:true}).click();
  const dialog=page.getByRole("dialog",{name:"教程附件",exact:true});
  await dialog.getByRole("radio",{name:"选择目录",exact:true}).check();
  const directory=resolve('../Cache/tests/tutorial-attachments/web-picker',String(Date.now()));
  await mkdir(directory,{recursive:true});
  const {writeFile}=await import('node:fs/promises');
  await writeFile(resolve(directory,'single.html'),'<h1>Any project</h1>');
  await dialog.getByLabel('选择教程目录',{exact:true}).setInputFiles(directory);
  await dialog.getByRole('button',{name:'确认上传并生效'}).click();
  await expect(dialog.getByRole('cell',{name:'single.html',exact:true})).toBeVisible();
  await mkdir(resolve(directory,'assets'),{recursive:true});await writeFile(resolve(directory,'assets/inner.js'),'void 0');
  await dialog.getByLabel('选择教程目录',{exact:true}).setInputFiles(directory);
  await expect(dialog.getByRole('alert')).toContainText('不含子目录');
});
