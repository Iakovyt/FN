// Shared types mirroring the serde structs on the Rust side
// (serde rename_all = "camelCase").

export interface ZapretConfig {
  strategyId: string;
  folderPath: string | null;
  gamingMode: boolean;
  autoUpdate: boolean;
  autoIpset: boolean;
  enabled: boolean;
}

export interface TgwsConfig {
  host: string;
  port: number;
  secret: string;
  enabled: boolean;
}

export interface AppConfig {
  zapret: ZapretConfig;
  tgws: TgwsConfig;
}

export interface StrategyInfo {
  id: string;
  name: string;
  /** True for the low-latency "gaming" variant. */
  gaming: boolean;
  /** The special auto-probe pseudo-strategy. */
  auto: boolean;
}

export interface FolderSelection {
  folderPath: string;
  strategyId: string;
  strategies: StrategyInfo[];
}

export interface Stats {
  activeModules: number;
  trafficBytesPerSec: number;
  uptimeSecs: number;
}

export type ModuleName = "zapret" | "tgws";

export interface ModuleStatus {
  module: ModuleName;
  running: boolean;
  /** Optional short detail, e.g. the strategy being probed. */
  detail?: string | null;
}

export interface LogLine {
  module: ModuleName;
  line: string;
}

export type ToastKind = "info" | "success" | "error";

export interface ToastPayload {
  kind: ToastKind;
  message: string;
}

/** Result of the install/bootstrap check for the zapret binaries. */
export interface InstallStatus {
  installed: boolean;
  /** Human-readable stage, surfaced while downloading. */
  stage: string;
}
