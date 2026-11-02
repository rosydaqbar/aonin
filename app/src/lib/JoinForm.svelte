<script lang="ts">
  /** Join a private listener by typing the code it shows. */
  import { actions } from './actions';
  import { app, normalizeCode } from './state.svelte';

  let { oncancel }: { oncancel: () => void } = $props();

  let typed = $state('');
  let input: HTMLInputElement;
  let valid = $derived(normalizeCode(typed) !== null);

  $effect(() => {
    input.focus();
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    const code = normalizeCode(typed);
    if (code && !app.joining) actions.joinCode(code);
  }

  function cancel() {
    actions.cancelJoin();
    oncancel();
  }
</script>

<form class="join" onsubmit={submit}>
  <h3>Join a private listener</h3>
  <label class="field-wrap">
    <span class="label">Type the code shown on the listener.</span>
    <input
      class="field code"
      bind:this={input}
      bind:value={typed}
      oninput={() => (app.joinError = null)}
      placeholder="K7Q 4MZ"
      maxlength="9"
      autocomplete="off"
      autocapitalize="characters"
      spellcheck="false"
      aria-invalid={app.joinError ? 'true' : undefined}
      aria-describedby="join-status"
    />
  </label>
  <p id="join-status" class="status" class:bad={app.joinError && !app.joining} role={app.joinError ? 'alert' : 'status'}>
    {#if app.joining}Looking for the listener…{:else if app.joinError}{app.joinError}{/if}
  </p>
  <div class="actions">
    <button type="button" class="btn quiet" onclick={cancel}>Cancel</button>
    <button class="btn primary" disabled={!valid || app.joining}>{app.joining ? 'Joining…' : 'Join'}</button>
  </div>
</form>

<style>
  .join { display: flex; flex-direction: column; }
  h3 { font-size: var(--size); font-weight: 700; margin-bottom: 12px; }
  .field-wrap { display: grid; gap: 6px; }
  .code { height: 40px; font-size: 18px; text-transform: uppercase; }
  .code::placeholder { color: var(--line-strong); }
  .status { min-height: 18px; margin-top: 6px; font-size: var(--size-s); color: var(--ink-2); }
  .status.bad { color: var(--red); }
  .actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 8px; }
</style>
