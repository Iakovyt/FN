<script lang="ts">
  import { toasts } from "../stores";
</script>

<div class="toast-stack">
  {#each $toasts as t (t.id)}
    <div
      class="toast {t.kind}"
      role="button"
      tabindex="0"
      on:click={() => toasts.dismiss(t.id)}
      on:keydown={(e) => (e.key === "Enter" || e.key === " ") && toasts.dismiss(t.id)}
    >
      <span class="glyph">
        {#if t.kind === "error"}✕{:else if t.kind === "success"}✓{:else}ℹ{/if}
      </span>
      <span class="msg">{t.message}</span>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 50;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 11px;
    border-radius: 10px;
    font-size: 11.5px;
    color: var(--text);
    background: rgba(20, 22, 40, 0.92);
    backdrop-filter: blur(12px);
    border: 0.5px solid var(--border);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    cursor: pointer;
    animation: slide 0.18s ease;
  }
  .toast.error {
    border-color: rgba(224, 109, 122, 0.5);
  }
  .toast.success {
    border-color: rgba(93, 202, 165, 0.5);
  }
  .glyph {
    flex-shrink: 0;
    font-size: 12px;
    width: 16px;
    text-align: center;
  }
  .toast.error .glyph {
    color: var(--danger);
  }
  .toast.success .glyph {
    color: var(--green);
  }
  .toast.info .glyph {
    color: var(--text-icon);
  }
  .msg {
    line-height: 1.35;
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
