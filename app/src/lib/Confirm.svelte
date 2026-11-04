<script lang="ts">
  /** A modal question; shown while mounted, `onclose` fires on either answer. */
  let {
    title,
    body,
    action,
    onconfirm,
    onclose,
  }: { title: string; body: string; action: string; onconfirm: () => void; onclose: () => void } = $props();
  import { t } from './i18n/index.svelte';
  let dialog: HTMLDialogElement;

  $effect(() => {
    if (!dialog.open) dialog.showModal();
  });
</script>

<dialog bind:this={dialog} {onclose} aria-labelledby="confirm-title">
  <h2 id="confirm-title">{title}</h2>
  <p>{body}</p>
  <form method="dialog">
    <button class="btn quiet">{t('common.cancel')}</button>
    <button class="btn primary" onclick={onconfirm}>{action}</button>
  </form>
</dialog>

<style>
  dialog {
    /* The global reset zeroes margins, which would pin the dialog top-left. */
    margin: auto;
    width: min(340px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 18px;
    background: var(--raised);
    color: var(--ink);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.25);
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.4); }
  h2 { font-size: var(--size); font-weight: 700; margin-bottom: 6px; }
  p { color: var(--ink-2); line-height: 1.5; }
  form { display: flex; justify-content: flex-end; gap: 6px; margin-top: 16px; }
</style>
