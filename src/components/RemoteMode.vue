<script setup lang="ts">
import Workspace from "../../shared/Workspace.vue";
import { api } from "../api/remote";
import {
  type NativeUpload,
  type FileManifestEntry,
  type VersionEntry,
  type NativeFile,
  type AttachmentResult,
  type ProjectConfig,
  resourcePath,
} from "../../shared/api";
import { open } from "@tauri-apps/plugin-dialog";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { invoke } from "@tauri-apps/api/core";
import { compareManifests } from "../../shared/uploadDiff";
import { readApkDirectory } from "../api/apk";
const native: NativeUpload = {
  credentials: {
    get: (id, url, username) => invoke("get_connection_password", {id, url, username}),
    save: (id, url, username, password) => invoke("save_connection_password", {id, url, username, password}),
    remove: (id) => invoke("remove_connection_password", {id}),
  },
  async chooseFiles(kind) {
    const selected = await open({multiple: kind === "files", directory: false, title: kind === "apk" ? "选择 APK" : kind === "catalog" ? "选择 catalog.json" : "选择下载文件", filters: kind === "files" ? undefined : [{name: kind === "apk" ? "APK" : "JSON", extensions: [kind === "apk" ? "apk" : "json"]}]});
    if (!selected) return [];
    return invoke<NativeFile[]>("describe_upload_files", {paths: Array.isArray(selected) ? selected : [selected]});
  },
  async uploadFiles(files, project, projectVersion, version) {
    return invoke<{version: string}>("upload_files_to_remote", {paths: files.map(file => file.path), projectType: project.project_type, projectId: project.id, projectVersion, version, serverUrl: api.getBaseUrl(), token: api.getToken()});
  },
  async uploadAttachments(files, project, projectVersion, revision) {
    return invoke<AttachmentResult<ProjectConfig>>("upload_remote_attachments", {paths: files.map(file => file.path), projectId: project.id, projectVersion, revision, serverUrl: api.getBaseUrl(), token: api.getToken()});
  },
  readAttachmentDirectory: directory => invoke<NativeFile[]>("read_attachment_directory", {directory}),
  apkReleases: readApkDirectory,
  async uploadApkRelease(directory, release, project, projectVersion) {
    return invoke<{version: string}>("upload_apk_release_to_remote", {directory, version: release.version, fingerprint: release.catalog_sha256, projectId: project.id, projectVersion, serverUrl: api.getBaseUrl(), token: api.getToken()});
  },
  async chooseDirectory(title = "选择 Bundles 目录") {
    return (await open({
      directory: true,
      multiple: false,
      title,
    })) as string | null;
  },
  versions(directory, project, platform) {
    return invoke<VersionEntry[]>("list_local_bundle_versions", {
      bundlesDir: directory,
      packageName: project.package_name,
      platform,
    });
  },
  manifest(directory, project, platform, version) {
    return invoke<FileManifestEntry[]>("compute_local_manifest", {
      bundlesDir: directory,
      packageName: project.package_name,
      platform,
      version,
    });
  },
  async upload(
    directory,
    project,
    projectVersion,
    platform,
    version,
    base,
    incremental,
  ) {
    const args = {
      bundlesDir: directory,
      packageName: project.package_name,
      platform,
      version,
      projectId: project.id,
      projectVersion,
      serverUrl: api.getBaseUrl(),
      token: api.getToken(),
    };
    if (incremental && base) {
      const [local, remote] = await Promise.all([
        invoke<FileManifestEntry[]>("compute_local_manifest", {
          bundlesDir: directory,
          packageName: project.package_name,
          platform,
          version,
        }),
        api.request<FileManifestEntry[]>(
          `${resourcePath(project, projectVersion)}/manifest?platform=${encodeURIComponent(platform)}&version=${encodeURIComponent(base)}`,
        ),
      ]);
      const diff = compareManifests(local, remote, base);
      const copyFiles = diff.unchanged.map((file) => file.name);
      const uploadFiles = [...diff.added, ...diff.modified].map((file) => file.name);
      await invoke("incremental_upload_to_remote", {
        ...args,
        baseVersion: base,
        copyFiles,
        uploadFiles,
      });
    } else await invoke("upload_version_to_remote", args);
  },
  openUrl: openExternal,
};
</script>
<template><Workspace :api="api" :native="native" /></template>
