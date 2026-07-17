<script lang="ts">
  import { tick } from "svelte";

  export let lines: string[] = [];
  export let title = "Лог";

  let open = false;
  let box: HTMLDivElement | undefined;

  async function toggleLog() {
    open = !open;
    if (!open) return;
    await tick();
    if (box) box.scrollTop = box.scrollHeight;
  }
</script>

<div class="logwrap">
  <button
    type="button"
    class="loghead"
    aria-expanded={open}
    on:click|stopPropagation={toggleLog}
  >
    <span class="chev" class:open>▸</span>
    <span>{title}</span>
    <span class="count">{lines.length}</span>
  </button>
  {#if open}
    <div class="logbox" bind:this={box}>
      {#if lines.length === 0}
        <div class="empty">— нет записей —</div>
      {:else}
        {#each lines as line}
          <div class="line">{line}</div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .logwrap {
    margin-top: 8px;
  }
  .loghead {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 11px;
    padding: 2px 0;
    cursor: pointer;
  }
  .chev {
    display: inline-block;
    transition: transform 0.15s ease;
    font-size: 9px;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .count {
    margin-left: auto;
    background: rgba(127, 119, 221, 0.18);
    border-radius: 10px;
    padding: 1px 7px;
    font-size: 10px;
  }
  .logbox {
    margin-top: 6px;
    max-height: 108px;
    overflow-y: auto;
    overscroll-behavior: contain;
    contain: content;
    background: rgba(0, 0, 0, 0.28);
    border: 0.5px solid var(--border-soft);
    border-radius: 8px;
    padding: 7px 9px;
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 10.5px;
    line-height: 1.5;
    color: #b8b4d8;
  }
  .line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .empty {
    color: var(--text-muted);
    font-style: italic;
  }
</style>
