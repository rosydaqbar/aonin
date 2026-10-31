<script lang="ts">
  import { Volume2, VolumeX } from '@lucide/svelte';
  import Grille from './Grille.svelte';
  import StatusMark from './StatusMark.svelte';
  import { fmtMs, type ClientRow } from './state.svelte';
  import { actions } from './actions';

  let { row, wide = false }: { row: ClientRow; wide?: boolean } = $props();

  let what = $derived(
    row.mark === 'connecting'
      ? 'Connecting…'
      : [row.sends.system && 'System audio', row.sends.mic && 'mic'].filter(Boolean).join(' + ')
  );
  let health = $derived(
    row.mark === 'muted' ? 'Muted'
    : row.mark === 'connecting' ? 'Connecting…'
    : fmtMs(row.latencyMs)
  );
  let note = $derived(row.mark !== 'degraded' ? null : wide ? `Unstable · ${row.recovered} repaired` : 'Unstable');
</script>

<div class="row" class:wide>
  <StatusMark mark={row.muted ? 'muted' : row.mark} />
  <div class="who">
    <span class="name" title={row.name}>{row.name}</span>
    {#if wide}
      <span class="label">{note ?? what}</span>
    {:else}
      <span class="label num" class:warn={row.mark === 'degraded'}>{note ?? health}</span>
    {/if}
  </div>
  <Grille level={row.level} recovered={row.recovered} holes={wide ? 18 : 10} muted={row.muted} />
  {#if wide}<span class="num lat" class:warn={row.mark === 'degraded'}>{health}</span>{/if}
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
  {#if wide}
    <button class="btn" class:dark={row.muted} onclick={() => actions.setMuted(row.id, !row.muted)} aria-pressed={row.muted}>
      {row.muted ? 'Muted' : 'Mute'}
    </button>
  {:else}
    <button
      class="icon"
      class:on={row.muted}
      onclick={() => actions.setMuted(row.id, !row.muted)}
      aria-pressed={row.muted}
      aria-label={row.muted ? `Unmute ${row.name}` : `Mute ${row.name}`}
    >
      {#if row.muted}<VolumeX size={15} strokeWidth={1.75} />{:else}<Volume2 size={15} strokeWidth={1.75} />{/if}
    </button>
  {/if}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto 64px 28px;
    align-items: center;
    gap: 10px;
    min-height: var(--row);
    border-bottom: 1px solid var(--line);
  }
  .row.wide {
    grid-template-columns: 8px minmax(0, 1fr) auto 64px 132px 64px;
    gap: 16px;
    min-height: 52px;
  }
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
  .icon:hover { border-color: var(--ink-2); }
  .icon.on { background: var(--strip); border-color: var(--strip); color: var(--strip-ink); }
  .btn { justify-content: center; }
</style>
