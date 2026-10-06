import type { ProjectType } from "../../shared/api";

export interface LocalProjectConfig {
  id: string;
  project_name: string;
  project_type?: ProjectType;
  bundles_dir: string;
  port: number;
  platforms: string[];
  cors_enabled: boolean;
  package_name: string;
  project_version: string;
  platform_access: Record<string, boolean>;
  active_releases?: Record<string, string>;
  published_releases?: string[];
  attachment_snapshot?: string | null;
}
