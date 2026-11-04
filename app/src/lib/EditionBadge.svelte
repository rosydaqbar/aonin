<script lang="ts">
  import { app } from './state.svelte';
  import { t } from './i18n/index.svelte';
</script>

<!-- Which Aonin this is, next to the computer name in the dark strip. -->
{#if app.proUnlocked}
  <span class="pro-tag lg" title={t('edition.proTitle', { version: app.version })}>Pro</span>
{:else}
  <span class="free" title={t('edition.freeTitle', { version: app.version })}>{t('edition.free')}</span>
{/if}

<style>
  .free {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: 18px;
    padding: 0 7px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, var(--strip-ink) 22%, transparent);
    color: var(--strip-ink-2);
    font-size: 10px;
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.04em;
  }
</style>
