<script lang="ts">
  import type { Component } from 'svelte';
  let {
    checked = $bindable(false),
    label,
    hint,
    icon: Icon,
    disabled = false,
    tag,
    onchange,
    onlocked,
  }: { checked?: boolean; label: string; hint?: string; icon?: Component<{ size?: number; strokeWidth?: number }>; disabled?: boolean; tag?: string; onchange?: (checked: boolean) => void; onlocked?: () => void } = $props();
</script>

<label class="switch" class:disabled>
  {#if Icon}<span class="icon" aria-hidden="true"><Icon size={16} strokeWidth={1.75} /></span>{/if}
  <span class="text">
    <span class="title">{label}{#if tag}<span class="pro-tag">{tag}</span>{/if}</span>
    {#if hint}<span class="label">{hint}</span>{/if}
  </span>
  <input type="checkbox" role="switch" bind:checked {disabled} onclick={(e) => { if (onlocked) { e.preventDefault(); onlocked(); } }} onchange={() => onchange?.(checked)} />
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
  .icon { display: grid; place-items: center; width: 28px; height: 28px; border-radius: 7px; background: var(--recess); color: var(--ink-2); flex: none; align-self: flex-start; margin-top: 12px; }
  .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .title { display: flex; align-items: center; gap: 6px; }
  input { position: absolute; opacity: 0; width: 1px; height: 1px; }
  .track {
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--ink-2);
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
    background: var(--ink-2);
    transition: transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  input:checked + .track { background: var(--hole-fill); box-shadow: none; }
  input:checked + .track .knob { background: var(--raised); box-shadow: 0 1px 2px rgb(0 0 0 / 0.25); }
  input:checked + .track .knob { transform: translateX(14px); }
  input:focus-visible + .track { outline: 2px solid var(--focus); outline-offset: 2px; }
  .disabled { opacity: 0.5; }
</style>
