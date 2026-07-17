// Thin, typed wrappers around Tauri commands + event listeners.
// The frontend never spawns processes itself — every side effect goes
// through a Rust command here.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  FolderSelection,
  InstallStatus,
  LogLine,
  ModuleStatus,
  Stats,
  StrategyInfo,
  ToastPayload,
} from "./types";

// ---- Config -------------------------------------------------------------

export const getConfig = () => invoke<AppConfig>("get_config");

export const getAutostart = () => invoke<boolean>("get_autostart");

export const setAutostart = (enabled: boolean) =>
  invoke<void>("set_autostart", { enabled });

// ---- Zapret -------------------------------------------------------------

export const listStrategies = () => invoke<StrategyInfo[]>("list_strategies");

export const ensureZapretInstalled = () =>
  invoke<InstallStatus>("ensure_zapret_installed");

export const setZapretFolder = (path: string) =>
  invoke<FolderSelection>("set_zapret_folder", { path });

export const zapretStart = (strategyId: string) =>
  invoke<void>("zapret_start", { strategyId });

export const zapretStop = () => invoke<void>("zapret_stop");

export const setStrategy = (strategyId: string) =>
  invoke<void>("set_strategy", { strategyId });

export const setGamingMode = (enabled: boolean) =>
  invoke<void>("set_gaming_mode", { enabled });

export const setAutoUpdate = (enabled: boolean) =>
  invoke<void>("set_auto_update", { enabled });

export const setAutoIpset = (enabled: boolean) =>
  invoke<void>("set_auto_ipset", { enabled });

/** Append an IP/CIDR to the active ipset file and hot-reload zapret. */
export const addIpsetEntry = (entry: string) =>
  invoke<void>("add_ipset_entry", { entry });

/** Add a domain to list-general.txt or an IP/CIDR to the active ipset. */
export const addZapretEntry = (entry: string) =>
  invoke<string>("add_zapret_entry", { entry });

// ---- TGWS ---------------------------------------------------------------

export const tgwsStart = () => invoke<void>("tgws_start");
export const tgwsStop = () => invoke<void>("tgws_stop");

export const setTgwsEndpoint = (host: string, port: number) =>
  invoke<void>("set_tgws_endpoint", { host, port });

export const openTelegramProxy = () => invoke<void>("open_telegram_proxy");

export const refreshTgwsSecret = () => invoke<string>("refresh_tgws_secret");

// ---- Stats / logs -------------------------------------------------------

export const getStats = () => invoke<Stats>("get_stats");

export const getLogs = (module: string) =>
  invoke<string[]>("get_logs", { module });

// ---- Events -------------------------------------------------------------

export const onModuleStatus = (cb: (s: ModuleStatus) => void): Promise<UnlistenFn> =>
  listen<ModuleStatus>("module-status", (e) => cb(e.payload));

export const onLogLine = (cb: (l: LogLine) => void): Promise<UnlistenFn> =>
  listen<LogLine>("log-line", (e) => cb(e.payload));

export const onStats = (cb: (s: Stats) => void): Promise<UnlistenFn> =>
  listen<Stats>("stats", (e) => cb(e.payload));

export const onToast = (cb: (t: ToastPayload) => void): Promise<UnlistenFn> =>
  listen<ToastPayload>("toast", (e) => cb(e.payload));
