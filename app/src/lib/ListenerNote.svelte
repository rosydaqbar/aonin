<script lang="ts">
  /** Pro belongs on the listener: say so wherever a key is bought or entered. */
  import { Speaker } from '@lucide/svelte';
  import { t } from './i18n/index.svelte';
</script>

<p class="note">
  <span class="icon" aria-hidden="true"><Speaker size={14} strokeWidth={1.75} /></span>
  <span class="text">
    <span class="title">{t('pro.listenerTitle')}</span>
    <span class="label">{t('pro.listenerBody')}</span>
  </span>
</p>

<style>
  .note { display: flex; gap: 10px; align-items: flex-start; align-self: stretch; padding: 10px 12px; border-radius: 9px; background: var(--recess); }
  .icon { display: grid; place-items: center; width: 24px; height: 24px; border-radius: 6px; background: var(--raised); color: var(--ink-2); flex: none; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .title { font-weight: 600; }
</style>
