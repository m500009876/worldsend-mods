import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface ModEntry {
  fileName: string;
  modId: string;
  displayName: string;
  version: string;
  downloadUrl: string;
  sha256: string;
  fileSizeBytes: number;
  required: boolean;
}

export interface Manifest {
  version: string;
  mcVersion: string;
  loader: string;
  loaderVersion: string;
  neoForgeUrl: string;
  publishedAt: string;
  mods: ModEntry[];
}

export interface NewsItem {
  id: string;
  title: string;
  content: string;
  tag: string;
  createdAt: string;
}

export interface ServerStatus {
  online: boolean;
  players?: { online: number; max: number };
  ping?: number;
}

export interface LaunchSettings {
  nickname: string;
  ramGb: number;
}

// Rust side uses snake_case (ram_gb) — these helpers translate at the edge
// so the rest of the frontend can stay in idiomatic camelCase.
function toRust(s: LaunchSettings) {
  return { nickname: s.nickname, ram_gb: s.ramGb };
}
function fromRust(s: { nickname: string; ram_gb: number }): LaunchSettings {
  return { nickname: s.nickname, ramGb: s.ram_gb };
}

export type LaunchProgress =
  | { stage: "Checking"; data: { message: string } }
  | { stage: "InstallingJava"; data: { message: string } }
  | { stage: "InstallingLoader"; data: { message: string } }
  | { stage: "SyncingMods"; data: { current: number; total: number; name: string } }
  | { stage: "DeletingMod"; data: { name: string } }
  | { stage: "Ready"; data?: undefined }
  | { stage: "Launching"; data?: undefined }
  | { stage: "Error"; data: { message: string } };

export const api = {
  getManifest: () => invoke<Manifest>("get_manifest"),
  getNews: () => invoke<NewsItem[]>("get_news"),
  getServerStatus: () => invoke<ServerStatus>("get_server_status"),
  checkJava: () => invoke<boolean>("check_java"),
  getSettings: async () => fromRust(await invoke<{ nickname: string; ram_gb: number }>("get_settings")),
  saveSettings: (s: LaunchSettings) => invoke<void>("save_settings", { settings: toRust(s) }),
  startLaunch: (s: LaunchSettings) => invoke<void>("start_launch", { settings: toRust(s) }),
  onLaunchProgress: (cb: (p: LaunchProgress) => void) =>
    listen<LaunchProgress>("launch-progress", (e) => cb(e.payload)),
};
