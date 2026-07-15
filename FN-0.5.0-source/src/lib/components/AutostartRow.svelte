<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { Power } from "lucide-svelte";
  import Toggle from "./Toggle.svelte";

  export let enabled = false;
  export let busy = false;

  const dispatch = createEventDispatcher<{ change: boolean }>();
</script>

<div class="autostart-row">
  <Power size={16} strokeWidth={1.8} />
  <span>Автозапуск</span>
  <Toggle on={enabled} disabled={busy} on:change={(event) => dispatch("change", event.detail)} />
</div>

<style>
  .autostart-row {
    height: 44px;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 10px;
    margin: 2px 0 10px;
    border-top: 0.5px solid var(--border);
    border-bottom: 0.5px solid var(--border);
    color: var(--text-muted);
  }
  .autostart-row span {
    flex: 1;
    color: var(--text);
    font-size: 12px;
    font-weight: 500;
  }
</style>
