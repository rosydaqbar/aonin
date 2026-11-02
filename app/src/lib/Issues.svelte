<script lang="ts">
  import { TriangleAlert } from '@lucide/svelte';
  import { actions } from './actions';
  import { app, type Issue } from './state.svelte';

  let { compact = false }: { compact?: boolean } = $props();

  const copy = (issue: Issue): { text: string; action: string } => {
    switch (issue) {
      case 'localNetwork':
        return { text: 'macOS is blocking Aonin from other computers on this network.', action: 'Open settings' };
      case 'audioCapture':
        return { text: "Aonin isn't allowed to record this Mac's audio.", action: 'Open settings' };
      case 'microphone':
        return app.me.os === 'macos'
          ? { text: "Aonin isn't allowed to use the microphone.", action: 'Open settings' }
          : { text: 'Windows privacy settings are blocking the microphone.', action: 'Open settings' };
      case 'firewall':
        return { text: 'Windows Firewall is blocking other devices from reaching this PC.', action: 'Allow in firewall' };
    }
  };
</script>

{#if app.issues.length}
  <div class="issues" class:compact role="status">
    {#each app.issues as issue (issue)}
      {@const c = copy(issue)}
      <div class="issue">
        <span class="mark" aria-hidden="true"><TriangleAlert size={15} strokeWidth={1.75} /></span>
        <p>{c.text}</p>
        {#if issue === 'localNetwork'}<button class="btn" onclick={actions.askLocalNetwork} title="Show the macOS prompt again">Ask again</button>{/if}
        <button class="btn primary" onclick={() => actions.fixIssue(issue)}>{c.action}</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .issues { display: flex; flex-direction: column; gap: 8px; margin-top: 16px; }
  .issue { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border-radius: 8px; background: var(--recess); }
  .mark { color: var(--yellow-ink); display: grid; flex: none; }
  .issue p { flex: 1; font-weight: 600; }
  .compact { margin: 0; padding: 10px 16px 0; }
  .compact .issue { gap: 10px; padding: 8px 10px; }
  .compact .issue p { font-size: var(--size-s); }
</style>
