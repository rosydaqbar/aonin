<script lang="ts">
  /** Unlock Pro: what Pro adds, then the key field. Shown while
   *  `proModal.step` is set. */
  import { X } from '@lucide/svelte';
  import ProFeatures from './ProFeatures.svelte';
  import { actions } from './actions';
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
      error = String(err).replace(/^Error: /, '');
    }
    busy = 'idle';
  }
</script>

<dialog bind:this={dialog} onclose={() => (proModal.step = null)} aria-labelledby="pro-title" onclick={(e) => e.target === dialog && close()}>
  <button class="x" onclick={close} aria-label="Close"><X size={15} strokeWidth={1.75} /></button>

  {#if app.proUnlocked}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">Aonin Pro is unlocked</h2>
    <p class="label">Everything in Pro is ready to use on this device.</p>
    <ProFeatures />
    <div class="actions"><button class="btn primary" onclick={close}>Done</button></div>
  {:else if app.proReady}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">Aonin Pro is ready</h2>
    <p class="label">Restart Aonin to finish. Your settings stay.</p>
    <div class="actions"><button class="btn quiet" onclick={close}>Later</button><button class="btn primary" onclick={actions.installUpdate}>Restart to finish</button></div>
  {:else if proModal.step === 'key' || app.license}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">Enter your license key</h2>
    {#if app.license}
      <p class="label">License {app.license.id}. {busy === 'downloading' ? 'Downloading Aonin Pro, this takes a moment.' : 'Download Aonin Pro to unlock its features.'}</p>
      {#if error}<p class="label bad" role="alert">{error}</p>{/if}
      <div class="actions"><button class="btn primary wide" onclick={() => unlock()} disabled={busy !== 'idle'}>{busy === 'downloading' ? 'Downloading…' : error ? 'Try again' : 'Download Aonin Pro'}</button></div>
    {:else}
      <p class="label">It’s in the email you got after buying. It starts with AONIN-.</p>
      <form class="keyform" onsubmit={unlock}>
        <input class="field" bind:this={input} bind:value={key} placeholder="AONIN-…" aria-label="License key" autocomplete="off" spellcheck="false" />
        {#if error}<p class="label bad" role="alert">{error}</p>{/if}
        <button class="btn primary wide" disabled={!key.trim() || busy !== 'idle'}>{busy === 'checking' ? 'Checking…' : busy === 'downloading' ? 'Downloading…' : 'Unlock'}</button>
      </form>
      <div class="foot">
        <span class="label">No key yet?</span>
        <button class="link" onclick={buy}>Purchase</button>
        <span class="label" aria-hidden="true">·</span>
        <button class="link" onclick={() => actions.openUrl(PRO_LINKS.request)}>Request a key</button>
      </div>
    {/if}
  {:else}
    <span class="pro-tag xl">Pro</span>
    <h2 id="pro-title">Unlock Pro features</h2>
    <p class="label">One license, yours for life, with a year of updates.</p>
    <ProFeatures />
    <div class="actions">
      <button class="btn primary wide" onclick={buy}>Purchase</button>
      <button class="btn wide" onclick={() => (proModal.step = 'key')}>Enter key</button>
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
