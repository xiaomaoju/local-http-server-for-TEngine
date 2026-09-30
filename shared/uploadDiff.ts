import type { FileManifestEntry } from "./api";

export interface UploadDiff {
  baseVersion: string | null;
  modified: FileManifestEntry[];
  added: FileManifestEntry[];
  unchanged: FileManifestEntry[];
  removed: FileManifestEntry[];
  totalSize: number;
  uploadSize: number;
}

export function compareManifests(local: FileManifestEntry[], remote: FileManifestEntry[], baseVersion: string | null): UploadDiff {
  const previous = new Map(remote.map((file) => [file.name, file]));
  const current = new Set(local.map((file) => file.name));
  const diff: UploadDiff = { baseVersion, modified: [], added: [], unchanged: [], removed: remote.filter((file) => !current.has(file.name)), totalSize: 0, uploadSize: 0 };
  for (const file of local) {
    const old = previous.get(file.name);
    if (!old) diff.added.push(file);
    else if (old.size === file.size && old.md5 === file.md5) diff.unchanged.push(file);
    else diff.modified.push(file);
    diff.totalSize += file.size;
  }
  diff.uploadSize = [...diff.added, ...diff.modified].reduce((size, file) => size + file.size, 0);
  return diff;
}
