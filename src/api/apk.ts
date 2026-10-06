import { invoke } from "@tauri-apps/api/core";
import type { ApkRelease } from "../../shared/api";
export const readApkDirectory = (directory: string) => invoke<ApkRelease[]>("list_local_apk_releases", {directory});
export const importApkRelease = (projectId: string, directory: string, release: ApkRelease) => invoke<string>("import_local_apk_release", {projectId, directory, version: release.version, fingerprint: release.catalog_sha256});
