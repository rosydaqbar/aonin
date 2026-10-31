<script lang="ts">
  import { untrack } from 'svelte';
  // The signature: a level meter drawn as a Braun speaker grille. Holes fill
  // with graphite as sound plays; a repaired packet lights one hole orange.
  let { level, recovered = 0, holes = 16, muted = false }: { level: number; recovered?: number; holes?: number; muted?: boolean } = $props();

  let lit = $derived(muted ? 0 : Math.round(Math.min(1, Math.max(0, level)) * holes));
  let flash = $state(-1);
  let seen = untrack(() => recovered);

  $effect(() => {
    if (recovered > seen) {
      flash = Math.max(0, lit - 1);
      const t = setTimeout(() => (flash = -1), 700);
      seen = recovered;
      return () => clearTimeout(t);
    }
    seen = recovered;
  });
</script>

<span class="grille" style:--n={holes} aria-hidden="true">
  {#each { length: holes } as _, i}
    <i class:on={i < lit} class:flash={i === flash}></i>
  {/each}
</span>

<style>
  .grille {
    display: grid;
    grid-template-columns: repeat(var(--n), 6px);
    gap: 3px;
    align-items: center;
  }
  i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--hole);
    box-shadow: inset 0 1px 1px rgb(0 0 0 / 0.12);
    transition: background 90ms linear;
  }
  i.on { background: var(--hole-fill); box-shadow: none; }
  i.flash { background: var(--orange); box-shadow: none; transition: none; }
  @media (prefers-reduced-motion: reduce) { i { transition: none; } }
</style>
