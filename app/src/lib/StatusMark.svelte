<script lang="ts">
  import type { Mark } from './state.svelte';
  import { t } from './i18n/index.svelte';

  let { mark }: { mark: Mark } = $props();

</script>

<span class="mark {mark}" role="img" aria-label={t(`mark.${mark}`)}></span>

<style>
  .mark {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    display: inline-block;
    flex: none;
  }
  .live { background: var(--green); }
  .muted { box-shadow: inset 0 0 0 1.5px var(--ink-2); }
  .connecting { background: var(--ink-2); animation: breathe 1.4s ease-in-out infinite; }
  .degraded { background: var(--yellow); }
  .error { background: var(--red); }

  @keyframes breathe { 50% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .connecting { animation: none; } }
</style>
