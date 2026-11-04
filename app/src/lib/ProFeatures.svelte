<script lang="ts">
  /** What Aonin Pro unlocks, upcoming parts included. */
  import { Check } from '@lucide/svelte';

  const features = [
    { title: 'Private listeners', detail: 'Only devices with the code can join.' },
    { title: 'Make listener', detail: 'Hand the listener role to another device.' },
    { title: 'Android and iOS apps', detail: 'Send and listen from your phone.', soon: true },
    { title: 'A year of updates', detail: 'Everything new in Pro during that year.' },
  ];
</script>

<ul>
  {#each features as f}
    <li>
      <span class="check" aria-hidden="true"><Check size={12} strokeWidth={2.5} /></span>
      <span class="text">
        <span class="title">{f.title}{#if f.soon}<span class="soon">Coming soon</span>{/if}</span>
        <span class="label">{f.detail}</span>
      </span>
    </li>
  {/each}
</ul>

<style>
  ul { list-style: none; padding: 0; margin: 0; display: grid; gap: 10px; }
  li { display: flex; gap: 10px; align-items: flex-start; }
  .check { display: grid; place-items: center; width: 18px; height: 18px; border-radius: 50%; background: var(--green-wash, color-mix(in srgb, var(--green) 18%, transparent)); color: var(--green); flex: none; margin-top: 1px; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .title { display: flex; align-items: center; gap: 6px; font-weight: 600; }
  .soon { display: inline-flex; align-items: center; height: 16px; padding: 0 6px; border-radius: 4px; border: 1px solid var(--line-strong); color: var(--ink-2); font-size: 10px; font-weight: 600; }
</style>
