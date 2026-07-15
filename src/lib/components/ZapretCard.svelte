<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Toggle from "./Toggle.svelte";
  import LogPanel from "./LogPanel.svelte";
  import type { StrategyInfo, ZapretConfig } from "../types";
  import { isValidIpOrCidr } from "../validation";

  export let config: ZapretConfig;
  export let strategies: StrategyInfo[] = [];
  export let running = false;
  export let busy = false;
  export let logs: string[] = [];

  const dispatch = createEventDispatcher<{
    toggle: boolean;
    strategy: string;
    gaming: boolean;
    autoUpdate: boolean;
    autoIpset: boolean;
    addIpset: string;
  }>();

  let ipsetValue = "";
  $: ipsetValid = ipsetValue.trim().length === 0 || isValidIpOrCidr(ipsetValue);

  function onStrategyChange(e: Event) {
    dispatch("strategy", (e.currentTarget as HTMLSelectElement).value);
  }

  function submitIpset() {
    const v = ipsetValue.trim();
    if (!v || !isValidIpOrCidr(v)) return;
    dispatch("addIpset", v);
    ipsetValue = "";
  }
</script>

<div class="card">
  <div class="card-head">
    <div class="card-head-left">
      <span class="icon">◈</span>
      <span>Запрет</span>
    </div>
    <Toggle on={running} disabled={busy} on:change={(e) => dispatch("toggle", e.detail)} />
  </div>

  <label class="small" for="strategy">Стратегия обхода</label>
  <select
    id="strategy"
    value={config.strategyId}
    disabled={busy}
    on:change={onStrategyChange}
  >
    {#each strategies as s}
      <option value={s.id}>{s.name}</option>
    {/each}
  </select>

  <div class="row">
    <span>🎮 Игровой режим</span>
    <Toggle on={config.gamingMode} on:change={(e) => dispatch("gaming", e.detail)} />
  </div>
  <div class="row">
    <span>🔄 Автообновление стратегий</span>
    <Toggle on={config.autoUpdate} on:change={(e) => dispatch("autoUpdate", e.detail)} />
  </div>
  <div class="row">
    <span>📋 Автодобавление IPset</span>
    <Toggle on={config.autoIpset} on:change={(e) => dispatch("autoIpset", e.detail)} />
  </div>

  <div class="ipset-row" class:invalid={!ipsetValid}>
    <input
      type="text"
      placeholder="Добавить IP / подсеть в IPset"
      bind:value={ipsetValue}
      on:keydown={(e) => e.key === "Enter" && submitIpset()}
      spellcheck="false"
      autocomplete="off"
    />
    <button
      class="add-btn"
      title="Добавить в IPset"
      disabled={!ipsetValue.trim() || !ipsetValid}
      on:click={submitIpset}
    >
      +
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
  select {
    width: 100%;
    background: rgba(255, 255, 255, 0.06);
    border: 0.5px solid var(--border-input);
    color: var(--text);
    font-size: 12px;
    border-radius: 8px;
    padding: 7px 8px;
    margin-bottom: 10px;
    appearance: none;
    outline: none;
    cursor: pointer;
    color-scheme: dark;
  }
  select option {
    background: #171a2a;
    color: var(--text);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .row span {
    color: var(--text-2);
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ipset-row {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }
  .ipset-row input {
    flex: 1;
    min-width: 0;
    background: rgba(255, 255, 255, 0.06);
    border: 0.5px solid var(--border-input);
    color: var(--text);
    font-size: 12px;
    border-radius: 8px;
    padding: 7px 8px;
    outline: none;
  }
  .ipset-row.invalid input {
    border-color: rgba(224, 109, 122, 0.6);
  }
  .add-btn {
    flex-shrink: 0;
    width: 32px;
    border-radius: 8px;
    background: rgba(127, 119, 221, 0.18);
    border: 0.5px solid var(--border-input);
    color: var(--text-icon);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }
  .add-btn:hover:not(:disabled) {
    background: rgba(127, 119, 221, 0.3);
  }
  .add-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
