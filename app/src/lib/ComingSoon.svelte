<script lang="ts">
  let { open = $bindable(false) }: { open?: boolean } = $props();
  let dialog: HTMLDialogElement;

  $effect(() => {
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} onclose={() => (open = false)} aria-labelledby="soon-title">
  <h2 id="soon-title">Private mode is coming soon</h2>
  <p>Only devices you pair with a code will be able to join.</p>
  <form method="dialog"><button class="btn primary">OK</button></form>
</dialog>

<style>
  dialog {
    /* The global reset zeroes margins, which would pin the dialog top-left. */
    margin: auto;
    width: min(300px, calc(100vw - 32px));
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
  form { display: flex; justify-content: flex-end; margin-top: 16px; }
</style>
