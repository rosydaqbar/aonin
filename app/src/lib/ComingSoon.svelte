<script lang="ts">
  import { Lock } from '@lucide/svelte';
  import { app } from './state.svelte';

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let dialog: HTMLDialogElement;

  $effect(() => {
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} onclose={() => (open = false)} aria-labelledby="soon-title">
  <span class="mark" aria-hidden="true"><Lock size={16} strokeWidth={1.75} /></span>
  <h2 id="soon-title">Private mode is coming soon</h2>
  <p>You'll be able to let in only the devices you pair with a one-time code, with their audio encrypted. Until then, anyone on this network can send audio to {app.me.name}.</p>
  <form method="dialog"><button class="btn primary">OK</button></form>
</dialog>

<style>
  dialog {
    width: min(320px, calc(100vw - 32px));
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 18px;
    background: var(--raised);
    color: var(--ink);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.18);
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.28); }
  .mark { display: grid; place-items: center; width: 30px; height: 30px; border-radius: 8px; background: var(--recess); color: var(--ink-2); }
  h2 { font-size: var(--size); font-weight: 700; margin: 12px 0 6px; }
  p { color: var(--ink-2); line-height: 1.5; }
  form { display: flex; justify-content: flex-end; margin-top: 16px; }
</style>
