<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { RefreshCw, Send } from "lucide-svelte";
  import Toggle from "./Toggle.svelte";
  import LogPanel from "./LogPanel.svelte";
  import type { TgwsConfig } from "../types";

  export let config: TgwsConfig;
  export let running = false;
  export let busy = false;
  export let logs: string[] = [];

  const dispatch = createEventDispatcher<{
    toggle: boolean;
    endpoint: { host: string; port: number };
    openTelegram: void;
    refreshSecret: void;
  }>();

  let host = config.host;
  let port = String(config.port);

  // Keep local fields in sync if the backing config changes elsewhere.
  $: host = config.host;
  $: port = String(config.port);

  $: portNum = parseInt(port, 10);
  $: portValid = Number.isInteger(portNum) && portNum > 0 && portNum <= 65535;
  $: hostValid = host.trim().length > 0;

  function commit() {
    if (!hostValid || !portValid) return;
    if (host.trim() === config.host && portNum === config.port) return;
    dispatch("endpoint", { host: host.trim(), port: portNum });
  }
</script>

<div class="card">
  <div class="card-head">
    <div class="card-head-left">
      <span class="icon">⇄</span>
      <span>TGWS Proxy</span>
    </div>
    <Toggle on={running} disabled={busy} on:change={(e) => dispatch("toggle", e.detail)} />
  </div>

  <label class="small" for="tgHost">Сервер</label>
  <input
    id="tgHost"
    type="text"
    bind:value={host}
    class:invalid={!hostValid}
    on:blur={commit}
    spellcheck="false"
    autocomplete="off"
  />

  <label class="small" for="tgPort">Порт</label>
  <input
    id="tgPort"
    type="text"
    inputmode="numeric"
    bind:value={port}
    class:invalid={!portValid}
    on:blur={commit}
    spellcheck="false"
    autocomplete="off"
  />

  <p class="status" class:on={running}>
    {#if running}
      {host}:{port} · активен
    {:else}
      остановлен
    {/if}
  </p>

  <div class="tg-actions">
    <button class="tg-btn" disabled={!hostValid || !portValid || busy} on:click={() => dispatch("openTelegram")}>
      <Send size={15} />
      Открыть в Telegram с прокси
    </button>
    <button
      class="refresh-btn"
      disabled={busy}
      title="Обновить secret"
      aria-label="Обновить secret"
      on:click={() => dispatch("refreshSecret")}
    >
      <RefreshCw size={15} class={busy ? "spinning" : ""} />
    </button>
  </div>

  <LogPanel lines={logs} title="Лог процесса" />
</div>

<style>
  .card {
    position: relative;
    background: var(--card);
    backdrop-filter: blur(10px);
    border: 0.5px solid var(--border);
    border-radius: 12px;
    padding: 14px;
    margin-bottom: 10px;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .card-head-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-head-left span:last-child {
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
  }
  .icon {
    font-size: 15px;
    color: var(--text-icon);
  }
  .small {
    color: var(--text-muted);
    font-size: 11px;
    display: block;
    margin-bottom: 4px;
  }
  input {
    width: 100%;
    background: rgba(255, 255, 255, 0.06);
    border: 0.5px solid var(--border-input);
    color: var(--text);
    font-size: 12px;
    border-radius: 8px;
    padding: 7px 8px;
    margin-bottom: 8px;
    outline: none;
  }
  input.invalid {
    border-color: rgba(224, 109, 122, 0.6);
  }
  .status {
    color: var(--text-muted);
    font-size: 11px;
    margin: 0 0 10px;
  }
  .status.on {
    color: var(--green-soft);
  }
  .tg-btn {
    flex: 1;
    background: rgba(93, 202, 165, 0.15);
    border: 0.5px solid rgba(93, 202, 165, 0.35);
    color: var(--green-soft);
    font-size: 12px;
    border-radius: 8px;
    padding: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    cursor: pointer;
  }
  .tg-btn:hover:not(:disabled) {
    background: rgba(93, 202, 165, 0.22);
  }
  .tg-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .tg-actions {
    display: flex;
    align-items: stretch;
    gap: 6px;
  }
  .refresh-btn {
    width: 38px;
    min-width: 38px;
    height: 36px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0.5px solid var(--border-input);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-muted);
    cursor: pointer;
  }
  .refresh-btn:hover:not(:disabled) {
    color: var(--text);
    background: rgba(255, 255, 255, 0.1);
  }
  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  :global(.spinning) {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
