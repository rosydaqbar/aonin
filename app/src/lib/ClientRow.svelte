<script lang="ts">
  import { ArrowLeftRight, Volume2, VolumeX } from '@lucide/svelte';
  import Grille from './Grille.svelte';
  import StatusMark from './StatusMark.svelte';
  import { fmtMs, type ClientRow } from './state.svelte';
  import { actions } from './actions';

  /** `lost`: the host's output device is gone, so nothing is actually playing. */
  /** `onmake`: offer to make this device the listener; `making`: that offer is open. */
  let { row, wide = false, lost = false, onmake, making = false }: { row: ClientRow; wide?: boolean; lost?: boolean; onmake?: () => void; making?: boolean } = $props();

  let what = $derived(
    row.mark === 'connecting'
      ? 'Connecting…'
      : [row.sends.system && 'System audio', row.sends.mic && 'mic'].filter(Boolean).join(' + ')
  );
  // Muting never hides whether the device is still connected and healthy.
  let latency = $derived(row.mark === 'connecting' ? 'Connecting…' : fmtMs(row.latencyMs));
  let health = $derived(row.muted ? `Muted · ${latency}` : latency);
  let note = $derived(row.mark !== 'degraded' ? null : wide ? `Unstable · ${row.recovered} repaired` : 'Unstable');
  let mark = $derived(lost ? 'connecting' : row.muted ? 'muted' : row.mark);
</script>

<div class="row" class:wide class:open={making}>
  <StatusMark {mark} />
  <div class="who">
    <span class="name" title={row.name}>{row.name}</span>
    {#if wide}
      <span class="label">{row.muted ? `Muted · ${what}` : (note ?? what)}</span>
    {:else}
      <span class="label num" class:warn={row.mark === 'degraded'}>{note ?? health}</span>
    {/if}
  </div>
  <Grille level={row.level} recovered={row.recovered} holes={wide ? 12 : 8} muted={row.muted || lost} />
  {#if wide}<span class="num lat" class:warn={row.mark === 'degraded'}>{latency}</span>{/if}
  <input
    type="range"
    min="0"
    max="1"
    step="0.01"
    value={row.volume}
    oninput={(e) => actions.setVolume(row.id, +e.currentTarget.value)}
    style:--p="{row.volume * 100}%"
    disabled={row.muted}
    aria-label="Volume for {row.name}"
  />
  <button
      class="icon"
      class:on={row.muted}
      onclick={() => actions.setMuted(row.id, !row.muted)}
      aria-pressed={row.muted}
      aria-label={row.muted ? `Unmute ${row.name}` : `Mute ${row.name}`}
    >
      {#if row.muted}<VolumeX size={15} strokeWidth={1.75} />{:else}<Volume2 size={15} strokeWidth={1.75} />{/if}
    </button>
  {#if wide}
    <button class="btn make" onclick={onmake} disabled={!onmake}><ArrowLeftRight size={13} strokeWidth={1.75} />Make listener</button>
  {:else}
    <button class="icon" class:active={making} onclick={onmake} disabled={!onmake} title="Make listener" aria-label="Make {row.name} the listener" aria-expanded={making}><ArrowLeftRight size={14} strokeWidth={1.75} /></button>
  {/if}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto 56px 28px 28px;
    align-items: center;
    gap: 8px;
    min-height: var(--row);
    border-bottom: 1px solid var(--line);
  }
  .row.wide {
    grid-template-columns: 8px minmax(120px, 1fr) auto 64px 100px 28px 124px;
    gap: 12px;
    min-height: 52px;
  }
  .row.open { border-bottom-color: transparent; }
  .who { display: flex; flex-direction: column; min-width: 0; }
  .name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lat { text-align: right; }
  .warn { color: var(--yellow-ink); }
  .icon {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 1px solid var(--line-strong);
    background: var(--raised);
    display: grid;
    place-items: center;
    color: var(--ink);
  }
  .icon:hover:not(:disabled) { border-color: var(--ink-2); }
  .icon:disabled { opacity: 0.45; }
  .icon.active { background: var(--ink); border-color: var(--ink); color: var(--housing); }
  .icon.on { background: var(--strip); border-color: var(--strip); color: var(--strip-ink); }
  .make { display: inline-flex; align-items: center; gap: 6px; padding: 0 12px; white-space: nowrap; justify-content: center; }
</style>
