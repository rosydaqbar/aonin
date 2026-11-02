<script lang="ts">
  /** A modal dialog around a form; shown while mounted. Esc or a click on
   *  the backdrop calls `onclose`. */
  import type { Snippet } from 'svelte';
  let { label, onclose, children }: { label: string; onclose: () => void; children: Snippet } = $props();
  let dialog: HTMLDialogElement;

  $effect(() => {
    if (!dialog.open) dialog.showModal();
  });
</script>

<dialog bind:this={dialog} {onclose} aria-label={label} onclick={(e) => e.target === dialog && dialog.close()}>
  {@render children()}
</dialog>

<style>
  dialog {
    /* The global reset zeroes margins, which would pin the dialog top-left. */
    margin: auto;
    width: min(380px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 18px;
    background: var(--raised);
    color: var(--ink);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.25);
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.4); }
</style>
