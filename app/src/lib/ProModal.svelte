<script lang="ts">
  /** Unlock Pro: what Pro adds, then the key field. Shown while
   *  `proModal.step` is set. */
  import { X } from '@lucide/svelte';
  import ListenerNote from './ListenerNote.svelte';
  import ProFeatures from './ProFeatures.svelte';
  import { actions } from './actions';
  import { t, tm } from './i18n/index.svelte';
  import { app, PRO_LINKS, proModal } from './state.svelte';

  let dialog: HTMLDialogElement;
  let input = $state<HTMLInputElement>();
  let key = $state('');
  let busy = $state<'idle' | 'checking' | 'downloading'>('idle');
  let error = $state<string | null>(null);

  $effect(() => {
    if (!dialog.open) dialog.showModal();
  });
  $effect(() => {
    if (proModal.step === 'key') input?.focus();
  });

  const close = () => dialog.close();
  const buy = () => actions.openUrl(PRO_LINKS.buy);

  async function unlock(e?: SubmitEvent) {
    e?.preventDefault();
    error = null;
    try {
      if (!app.license) {
        busy = 'checking';
        await actions.setLicense(key);
        key = '';
      }
      if (app.edition === 'free') {
        busy = 'downloading';
        await actions.unlockPro();
      }
    } catch (err) {
      error = tm(String(err));
    }
    busy = 'idle';
  }
</script>

<dialog bind:this={dialog} onclose={() => (proModal.step = null)} aria-labelledby="pro-title" onclick={(e) => e.target === dialog && close()}>
  <button class="x" onclick={close} aria-label={t('pro.close')}><X size={15} strokeWidth={1.75} /></button>

  {#if app.proUnlocked}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">{t('pro.unlocked')}</h2>
    <p class="label">{t('pro.unlockedHint')}</p>
    <ProFeatures />
    <div class="actions"><button class="btn primary" onclick={close}>{t('pro.done')}</button></div>
  {:else if app.proReady}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">{t('pro.ready')}</h2>
    <p class="label">{t('pro.readyHint')}</p>
    <div class="actions"><button class="btn quiet" onclick={close}>{t('pro.later')}</button><button class="btn primary" onclick={actions.installUpdate}>{t('pro.restartToFinish')}</button></div>
  {:else if proModal.step === 'key' || app.license}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">{t('pro.keyTitle')}</h2>
    {#if app.license}
      <p class="label">{t(busy === 'downloading' ? 'pro.downloadingHint' : 'pro.downloadHint', { id: app.license.id })}</p>
      {#if error}<p class="label bad" role="alert">{error}</p>{/if}
      <div class="actions"><button class="btn primary wide" onclick={() => unlock()} disabled={busy !== 'idle'}>{busy === 'downloading' ? t('pro.downloading') : error ? t('pro.tryAgain') : t('pro.download')}</button></div>
    {:else}
      <p class="label">{t('pro.keyHint')}</p>
      <ListenerNote />
      <form class="keyform" onsubmit={unlock}>
        <input class="field" bind:this={input} bind:value={key} placeholder="AONIN-…" aria-label={t('pro.keyAria')} autocomplete="off" spellcheck="false" />
        {#if error}<p class="label bad" role="alert">{error}</p>{/if}
        <button class="btn primary wide" disabled={!key.trim() || busy !== 'idle'}>{busy === 'checking' ? t('pro.checking') : busy === 'downloading' ? t('pro.downloading') : t('pro.unlock')}</button>
      </form>
      <div class="foot">
        <span class="label">{t('pro.noKey')}</span>
        <button class="link" onclick={buy}>{t('pro.purchase')}</button>
        <span class="label" aria-hidden="true">·</span>
        <button class="link" onclick={() => actions.openUrl(PRO_LINKS.request)}>{t('pro.requestKey')}</button>
      </div>
    {/if}
  {:else}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">{t('pro.unlockTitle')}</h2>
    <p class="label">{t('pro.offer')}</p>
    <ProFeatures />
    <ListenerNote />
    <div class="actions">
      <button class="btn primary wide" onclick={buy}>{t('pro.purchase')}</button>
      <button class="btn wide" onclick={() => (proModal.step = 'key')}>{t('pro.enterKey')}</button>
    </div>
  {/if}
</dialog>

<style>
  dialog {
    /* The global reset zeroes margins, which would pin the dialog top-left. */
    margin: auto;
    width: min(360px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 22px;
    background: var(--raised);
    color: var(--ink);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.25);
  }
  dialog[open] { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
  dialog::backdrop { background: rgb(0 0 0 / 0.4); }
  dialog > :global(ul) { margin: 12px 0 4px; align-self: stretch; }
  dialog > :global(.note) { margin-top: 8px; }
  .x { position: absolute; top: 12px; right: 12px; display: grid; place-items: center; width: 28px; height: 28px; border: 0; border-radius: 7px; background: none; color: var(--ink-2); cursor: pointer; }
  .x:hover { background: var(--recess); color: var(--ink); }
  h2 { font-size: 16px; font-weight: 700; margin-top: 10px; }
  .actions { display: flex; gap: 8px; margin-top: 14px; align-self: stretch; justify-content: flex-end; }
  .wide { flex: 1; justify-content: center; }
  .keyform .wide { flex: none; }
  .keyform { display: flex; flex-direction: column; gap: 8px; align-self: stretch; margin-top: 8px; }
  .keyform .field { font-family: var(--mono, ui-monospace, monospace); font-size: 12px; }
  .foot { display: flex; align-items: center; gap: 6px; margin-top: 12px; }
  .link { background: none; border: 0; padding: 0; font: inherit; color: var(--ink); text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }
  .bad { color: var(--red); }
</style>
