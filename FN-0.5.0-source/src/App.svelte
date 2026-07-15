<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { ChevronRight, FolderOpen } from "lucide-svelte";
  import ZapretCard from "./lib/components/ZapretCard.svelte";
  import TgwsCard from "./lib/components/TgwsCard.svelte";
  import AutostartRow from "./lib/components/AutostartRow.svelte";
  import StatsBar from "./lib/components/StatsBar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import { toasts, appStart } from "./lib/stores";
  import * as api from "./lib/api";
  import type { AppConfig, StrategyInfo, Stats } from "./lib/types";

  let config: AppConfig = {
    zapret: { strategyId: "auto", folderPath: null, gamingMode: false, autoUpdate: true, autoIpset: true, enabled: false },
    tgws: { host: "127.0.0.1", port: 2222, secret: "", enabled: false },
  };
  let strategies: StrategyInfo[] = [];
  let zapretRunning = false;
  let tgwsRunning = false;
  let zapretBusy = false;
  let tgwsBusy = false;
  let folderBusy = false;
  let autostartEnabled = false;
  let autostartBusy = false;
  let zapretLogs: string[] = [];
  let tgwsLogs: string[] = [];

  let stats: Stats = { activeModules: 0, trafficBytesPerSec: 0, uptimeSecs: 0 };

  const MAX_LOG = 20;
  const unlisteners: UnlistenFn[] = [];
  let uptimeTimer: ReturnType<typeof setInterval>;

  $: anyActive = zapretRunning || tgwsRunning;

  // Custom window controls (native decorations are disabled).
  const appWindow = getCurrentWindow();
  const minimizeWindow = () => appWindow.minimize();
  const hideToTray = () => appWindow.hide(); // keeps modules running in the tray

  // ---- Eased ("smooth") wheel scrolling for the content area ----------
  // CSS scroll-behavior only smooths programmatic/keyboard scrolls, so we
  // animate the wheel ourselves with a small rAF lerp, while still letting
  // nested scrollers (the log panel) consume the wheel first.
  let contentEl: HTMLDivElement;
  let targetY = 0;
  let rafId = 0;

  function nestedCanScroll(target: EventTarget | null, deltaY: number): boolean {
    let el = target as HTMLElement | null;
    while (el && el !== contentEl) {
      const oy = getComputedStyle(el).overflowY;
      if ((oy === "auto" || oy === "scroll") && el.scrollHeight > el.clientHeight) {
        const atTop = el.scrollTop <= 0;
        const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 1;
        if (!(deltaY < 0 && atTop) && !(deltaY > 0 && atBottom)) return true;
      }
      el = el.parentElement;
    }
    return false;
  }

  function onWheel(e: WheelEvent) {
    if (!contentEl || contentEl.scrollHeight <= contentEl.clientHeight) return;
    if (nestedCanScroll(e.target, e.deltaY)) return; // let the log panel scroll
    e.preventDefault();
    const max = contentEl.scrollHeight - contentEl.clientHeight;
    targetY = Math.max(0, Math.min(max, targetY + e.deltaY));
    if (!rafId) rafId = requestAnimationFrame(stepScroll);
  }

  function stepScroll() {
    const cur = contentEl.scrollTop;
    const diff = targetY - cur;
    if (Math.abs(diff) < 0.5) {
      contentEl.scrollTop = targetY;
      rafId = 0;
      return;
    }
    contentEl.scrollTop = cur + diff * 0.18;
    rafId = requestAnimationFrame(stepScroll);
  }

  function onContentScroll() {
    // Keep the target in sync when scrolled by other means (keyboard, drag).
    if (!rafId && contentEl) targetY = contentEl.scrollTop;
  }

  function errText(e: unknown): string {
    if (typeof e === "string") return e;
    if (e && typeof e === "object" && "message" in e) return String((e as any).message);
    return String(e);
  }

  onMount(async () => {
    try {
      config = await api.getConfig();
    } catch (e) {
      toasts.error(`Не удалось загрузить конфиг: ${errText(e)}`);
    }

    try {
      autostartEnabled = await api.getAutostart();
    } catch (e) {
      toasts.error(`Автозапуск: ${errText(e)}`);
    }

    // Event wiring.
    unlisteners.push(
      await api.onModuleStatus((s) => {
        if (s.module === "zapret") {
          zapretRunning = s.running;
        } else {
          tgwsRunning = s.running;
        }
      }),
      await api.onLogLine((l) => {
        if (l.module === "zapret") {
          zapretLogs = [...zapretLogs, l.line].slice(-MAX_LOG);
        } else {
          tgwsLogs = [...tgwsLogs, l.line].slice(-MAX_LOG);
        }
      }),
      await api.onStats((s) => (stats = s)),
      await api.onToast((t) => toasts.fromPayload(t)),
    );

    const [savedZapretLogs, savedTgwsLogs] = await Promise.all([
      api.getLogs("zapret"),
      api.getLogs("tgws"),
    ]);
    zapretLogs = savedZapretLogs.slice(-MAX_LOG);
    tgwsLogs = savedTgwsLogs.slice(-MAX_LOG);

    // Load strategies + bootstrap zapret binaries in the background.
    api
      .listStrategies()
      .then((s) => (strategies = s))
      .catch((e) => toasts.error(`Стратегии: ${errText(e)}`));

    api
      .ensureZapretInstalled()
      .then((st) => {
        if (!st.installed) toasts.info(st.stage);
      })
      .catch((e) => toasts.error(`Установка zapret: ${errText(e)}`));

    // Local uptime fallback so the counter moves even between backend ticks.
    uptimeTimer = setInterval(() => {
      const local = Math.floor((Date.now() - appStart) / 1000);
      if (local > stats.uptimeSecs) stats = { ...stats, uptimeSecs: local };
    }, 1000);
  });

  onDestroy(() => {
    unlisteners.forEach((u) => u());
    clearInterval(uptimeTimer);
    if (rafId) cancelAnimationFrame(rafId);
  });

  // ---- Zapret handlers --------------------------------------------------

  async function toggleZapret(on: boolean) {
    zapretBusy = true;
    try {
      if (on) {
        await api.zapretStart(config.zapret.strategyId);
        zapretRunning = true;
        toasts.success("Запрет запущен");
      } else {
        await api.zapretStop();
        zapretRunning = false;
        toasts.info("Запрет остановлен");
      }
      config.zapret.enabled = on;
    } catch (e) {
      toasts.error(errText(e));
      zapretRunning = !on ? zapretRunning : false;
    } finally {
      zapretBusy = false;
    }
  }

  async function changeStrategy(id: string) {
    config.zapret.strategyId = id;
    config = config; // force Svelte to see the nested change
    try {
      await api.setStrategy(id);
      if (zapretRunning) {
        zapretBusy = true;
        await api.zapretStart(id); // backend restarts with the new strategy
        toasts.success("Стратегия применена");
      }
    } catch (e) {
      toasts.error(errText(e));
    } finally {
      zapretBusy = false;
    }
  }

  async function setGaming(on: boolean) {
    config.zapret.gamingMode = on;
    config = config;
    try {
      await api.setGamingMode(on);
    } catch (e) {
      toasts.error(errText(e));
      config.zapret.gamingMode = !on;
      config = config;
    }
  }

  async function setAutoUpdate(on: boolean) {
    config.zapret.autoUpdate = on;
    config = config;
    try {
      await api.setAutoUpdate(on);
    } catch (e) {
      toasts.error(errText(e));
      config.zapret.autoUpdate = !on;
      config = config;
    }
  }

  async function setAutoIpset(on: boolean) {
    config.zapret.autoIpset = on;
    config = config;
    try {
      await api.setAutoIpset(on);
    } catch (e) {
      toasts.error(errText(e));
      config.zapret.autoIpset = !on;
      config = config;
    }
  }

  async function addIpset(entry: string) {
    try {
      await api.addIpsetEntry(entry);
      toasts.success(`Добавлено в IPset: ${entry}`);
    } catch (e) {
      toasts.error(errText(e));
    }
  }

  async function selectZapretFolder() {
    if (folderBusy) return;
    folderBusy = true;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Выберите папку Запрета",
      });
      if (typeof selected !== "string") return;
      const result = await api.setZapretFolder(selected);
      strategies = result.strategies;
      config.zapret.folderPath = result.folderPath;
      config.zapret.strategyId = result.strategyId;
      config.zapret.enabled = false;
      config = config;
      zapretRunning = false;
      const count = result.strategies.filter((strategy) => !strategy.auto).length;
      toasts.success(`Найдено стратегий: ${count}`);
    } catch (e) {
      toasts.error(errText(e));
    } finally {
      folderBusy = false;
    }
  }

  // ---- TGWS handlers ----------------------------------------------------

  async function toggleTgws(on: boolean) {
    tgwsBusy = true;
    try {
      if (on) {
        await api.tgwsStart();
        tgwsRunning = true;
        toasts.success("TGWS запущен");
      } else {
        await api.tgwsStop();
        tgwsRunning = false;
        toasts.info("TGWS остановлен");
      }
      config.tgws.enabled = on;
    } catch (e) {
      toasts.error(errText(e));
    } finally {
      tgwsBusy = false;
    }
  }

  async function setEndpoint(host: string, port: number) {
    config.tgws.host = host;
    config.tgws.port = port;
    config = config;
    try {
      await api.setTgwsEndpoint(host, port);
      if (tgwsRunning) toasts.info("Прокси перезапущен с новыми параметрами");
    } catch (e) {
      toasts.error(errText(e));
    }
  }

  async function openTelegram() {
    try {
      await api.openTelegramProxy();
    } catch (e) {
      toasts.error(errText(e));
    }
  }

  async function refreshTgwsSecret() {
    tgwsBusy = true;
    try {
      config.tgws.secret = await api.refreshTgwsSecret();
      config = config;
      toasts.success("Secret обновлён");
    } catch (e) {
      toasts.error(errText(e));
    } finally {
      tgwsBusy = false;
    }
  }

  async function toggleAutostart(enabled: boolean) {
    autostartBusy = true;
    try {
      await api.setAutostart(enabled);
      autostartEnabled = enabled;
      if (enabled) {
        toasts.success("Автозапуск включён");
      } else {
        toasts.info("Автозапуск отключён");
      }
    } catch (e) {
      toasts.error(errText(e));
    } finally {
      autostartBusy = false;
    }
  }
</script>

<main class="app">
  <div class="glow1"></div>
  <div class="glow2"></div>

  <header class="titlebar" data-tauri-drag-region>
    <div class="logo" data-tauri-drag-region>🛡</div>
    <span class="title" data-tauri-drag-region>FN</span>
    {#if anyActive}
      <span class="badge" data-tauri-drag-region><span class="dot"></span>активна</span>
    {/if}
    <div class="win-controls">
      <button class="win-btn" title="Свернуть" on:click={minimizeWindow} aria-label="Свернуть">
        <svg width="10" height="10" viewBox="0 0 10 10"><line x1="1" y1="5" x2="9" y2="5" /></svg>
      </button>
      <button class="win-btn close" title="Свернуть в трей" on:click={hideToTray} aria-label="Закрыть">
        <svg width="10" height="10" viewBox="0 0 10 10">
          <line x1="1" y1="1" x2="9" y2="9" /><line x1="9" y1="1" x2="1" y2="9" />
        </svg>
      </button>
    </div>
  </header>

  <div class="content" bind:this={contentEl} on:wheel={onWheel} on:scroll={onContentScroll}>
    <ZapretCard
      config={config.zapret}
      {strategies}
      running={zapretRunning}
      busy={zapretBusy}
      logs={zapretLogs}
      on:toggle={(e) => toggleZapret(e.detail)}
      on:strategy={(e) => changeStrategy(e.detail)}
      on:gaming={(e) => setGaming(e.detail)}
      on:autoUpdate={(e) => setAutoUpdate(e.detail)}
      on:autoIpset={(e) => setAutoIpset(e.detail)}
      on:addIpset={(e) => addIpset(e.detail)}
    />

    <TgwsCard
      config={config.tgws}
      running={tgwsRunning}
      busy={tgwsBusy}
      logs={tgwsLogs}
      on:toggle={(e) => toggleTgws(e.detail)}
      on:endpoint={(e) => setEndpoint(e.detail.host, e.detail.port)}
      on:openTelegram={openTelegram}
      on:refreshSecret={refreshTgwsSecret}
    />

    <StatsBar
      activeModules={stats.activeModules}
      trafficBytesPerSec={stats.trafficBytesPerSec}
      uptimeSecs={stats.uptimeSecs}
    />

    <button
      class="folder-action"
      class:busy={folderBusy}
      type="button"
      disabled={folderBusy}
      on:click={selectZapretFolder}
    >
      <span class="folder-icon"><FolderOpen size={17} strokeWidth={1.8} /></span>
      <span class="folder-copy">
        <strong>Выбрать папку Запрета</strong>
        <small>{config.zapret.folderPath || "%APPDATA%\\FN\\zapret"}</small>
      </span>
      <ChevronRight class="folder-chevron" size={16} strokeWidth={1.8} />
    </button>

    <AutostartRow enabled={autostartEnabled} busy={autostartBusy} on:change={(e) => toggleAutostart(e.detail)} />
  </div>

  <Toasts />
</main>

<style>
  .app {
    width: 380px;
    height: 100vh;
    background: var(--bg);
    position: relative;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .glow1 {
    position: absolute;
    top: -60px;
    right: -60px;
    width: 200px;
    height: 200px;
    background: #6d5ae6;
    opacity: 0.18;
    border-radius: 50%;
    pointer-events: none;
  }
  .glow2 {
    position: absolute;
    bottom: -80px;
    left: -40px;
    width: 180px;
    height: 180px;
    background: #3a2f8f;
    opacity: 0.15;
    border-radius: 50%;
    pointer-events: none;
  }
  /* ---- Custom draggable title bar (replaces native decorations) ---- */
  .titlebar {
    position: relative;
    z-index: 5;
    flex-shrink: 0;
    height: 42px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 14px;
    border-bottom: 0.5px solid rgba(127, 119, 221, 0.12);
    -webkit-user-select: none;
    user-select: none;
  }
  .logo {
    width: 26px;
    height: 26px;
    border-radius: 8px;
    background: linear-gradient(135deg, #7f77dd, #4a3fb5);
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    font-size: 13px;
    flex-shrink: 0;
    box-shadow: 0 2px 8px rgba(127, 119, 221, 0.35);
  }
  .title {
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 1px;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(29, 158, 117, 0.15);
    border: 0.5px solid rgba(93, 202, 165, 0.3);
    padding: 3px 9px;
    border-radius: 20px;
    color: var(--green-soft);
    font-size: 11px;
    margin-left: 6px;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--green);
    box-shadow: 0 0 6px var(--green);
  }
  .win-controls {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .win-btn {
    width: 30px;
    height: 28px;
    border: none;
    background: transparent;
    border-radius: 7px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .win-btn svg {
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
  }
  .win-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: rgba(224, 109, 122, 0.85);
    color: #fff;
  }

  /* ---- Scrollable content area with smooth scrolling ---- */
  .content {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 14px 16px 16px;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
  }
  .folder-action {
    width: 100%;
    min-height: 54px;
    margin-top: 10px;
    padding: 8px 11px;
    display: flex;
    align-items: center;
    gap: 10px;
    border: 1px solid rgba(127, 119, 221, 0.2);
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.035);
    color: var(--text-2);
    cursor: pointer;
    text-align: left;
    transition: background 0.16s ease, border-color 0.16s ease, transform 0.16s ease;
  }
  .folder-action:hover:not(:disabled) {
    background: rgba(127, 119, 221, 0.1);
    border-color: rgba(127, 119, 221, 0.38);
  }
  .folder-action:active:not(:disabled) {
    transform: translateY(1px);
  }
  .folder-action:focus-visible {
    outline: 2px solid rgba(127, 119, 221, 0.65);
    outline-offset: 2px;
  }
  .folder-action:disabled {
    cursor: wait;
    opacity: 0.65;
  }
  .folder-icon {
    width: 32px;
    height: 32px;
    flex: 0 0 32px;
    display: grid;
    place-items: center;
    border-radius: 7px;
    color: var(--text-icon);
    background: rgba(127, 119, 221, 0.12);
  }
  .folder-copy {
    min-width: 0;
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }
  .folder-copy strong {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
  }
  .folder-copy small {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(.folder-chevron) {
    flex: 0 0 auto;
    color: var(--text-muted);
    transition: transform 0.16s ease, color 0.16s ease;
  }
  .folder-action:hover:not(:disabled) :global(.folder-chevron) {
    color: var(--text-icon);
    transform: translateX(2px);
  }
</style>
