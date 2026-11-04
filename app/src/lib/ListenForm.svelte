<script lang="ts">
  /** Become the listener: the name audio senders see, and optionally a
   *  private session that only devices with the code can join. */
  import { Lock, RefreshCw } from '@lucide/svelte';
  import Switch from './Switch.svelte';
  import { actions } from './actions';
  import { app, fmtCode, newCode } from './state.svelte';

  let { oncancel, ondone }: { oncancel: () => void; ondone?: () => void } = $props();

  let name = $state(app.me.name);
  let priv = $state(false);
  let code = $state(newCode());
  let input: HTMLInputElement;

  $effect(() => {
    input.focus();
    input.select();
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    actions.startListening(name, priv ? code : null);
    ondone?.();
  }
</script>

<form class="listen" onsubmit={submit}>
  <h3>Become the listener</h3>
  <label class="name">
    <span class="label">Name</span>
    <input class="field" bind:this={input} bind:value={name} maxlength="40" placeholder={app.me.name} autocomplete="off" spellcheck="false" />
  </label>
  {#if app.proUnlocked}
    <Switch icon={Lock} label="Make private" hint="Only devices with the code can join. It won't show up for anyone else." bind:checked={priv} />
  {:else}
    <Switch icon={Lock} label="Make private" tag="Pro" hint="Only devices with the code can join. Needs Aonin Pro." disabled />
    <button type="button" class="link getpro" onclick={() => actions.openWindow('about')}>Get Aonin Pro</button>
  {/if}
  {#if priv}
    <div class="codebox">
      <span class="text">
        <span class="label">Code for audio senders</span>
        <span class="code big" aria-label="Code {code.split('').join(' ')}">{fmtCode(code)}</span>
      </span>
      <button type="button" class="btn quiet" onclick={() => (code = newCode())}><RefreshCw size={13} strokeWidth={1.75} />New code</button>
    </div>
  {/if}
  <div class="actions">
    <button type="button" class="btn quiet" onclick={oncancel}>Cancel</button>
    <button class="btn primary">Start listening</button>
  </div>
</form>

<style>
  .listen { display: flex; flex-direction: column; }
  h3 { font-size: var(--size); font-weight: 700; margin-bottom: 12px; }
  .name { display: grid; gap: 5px; padding-bottom: 6px; }
  .codebox { display: flex; align-items: center; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); }
  .codebox .text { flex: 1; display: flex; flex-direction: column; gap: 4px; }
  .big { font-size: 22px; user-select: text; cursor: text; }
  .actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 14px; }
  .getpro { align-self: flex-start; margin-top: 8px; background: none; border: 0; padding: 0; font: inherit; color: var(--ink); text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }
</style>
