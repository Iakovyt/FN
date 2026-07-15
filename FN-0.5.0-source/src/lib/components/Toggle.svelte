<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let on = false;
  export let disabled = false;

  const dispatch = createEventDispatcher<{ change: boolean }>();

  function toggle() {
    if (disabled) return;
    dispatch("change", !on);
  }
</script>

<button
  class="toggle"
  class:on
  class:disabled
  role="switch"
  aria-checked={on}
  {disabled}
  on:click|stopPropagation={toggle}
>
  <span class="knob" />
</button>

<style>
  .toggle {
    width: 32px;
    height: 17px;
    border-radius: 20px;
    background: rgba(255, 255, 255, 0.12);
    position: relative;
    cursor: pointer;
    transition: background 0.15s ease;
    flex-shrink: 0;
    border: none;
    padding: 0;
  }
  .toggle.on {
    background: var(--green);
  }
  .toggle.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .knob {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: #fff;
    position: absolute;
    top: 2px;
    left: 2px;
    transition: left 0.15s ease;
  }
  .toggle.on .knob {
    left: 17px;
  }
</style>
