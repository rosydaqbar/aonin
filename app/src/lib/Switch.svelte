<script lang="ts">
  let { checked = $bindable(false), label, hint, disabled = false, onchange }: { checked?: boolean; label: string; hint?: string; disabled?: boolean; onchange?: (checked: boolean) => void } = $props();
</script>

<label class="switch" class:disabled>
  <span class="text">
    <span>{label}</span>
    {#if hint}<span class="label">{hint}</span>{/if}
  </span>
  <input type="checkbox" role="switch" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span class="track" aria-hidden="true"><span class="knob"></span></span>
</label>

<style>
  .switch {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    border-bottom: 1px solid var(--line);
  }
  .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  input { position: absolute; opacity: 0; width: 1px; height: 1px; }
  .track {
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: var(--line-strong);
    position: relative;
    flex: none;
    transition: background 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  .knob {
    position: absolute;
    left: 2px;
    top: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--raised);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  input:checked + .track { background: var(--hole-fill); }
  input:checked + .track .knob { transform: translateX(14px); }
  input:focus-visible + .track { outline: 2px solid var(--focus); outline-offset: 2px; }
  .disabled { opacity: 0.5; }
</style>
