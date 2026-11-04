<script lang="ts">
  import { TriangleAlert } from '@lucide/svelte';
  import { actions } from './actions';
  import { t } from './i18n/index.svelte';
  import { app, localNet, type Issue } from './state.svelte';

  let { compact = false }: { compact?: boolean } = $props();

  const copy = (issue: Issue): { text: string; action: string } => {
    switch (issue) {
      case 'localNetwork':
        return localNet.ask === 'done'
          ? { text: t('issue.localNetworkDidntAsk'), action: t('common.openSettings') }
          : { text: t('issue.localNetwork'), action: t('common.openSettings') };
      case 'audioCapture':
        return { text: t('issue.audioCapture'), action: t('common.openSettings') };
      case 'microphone':
        return app.me.os === 'macos'
          ? { text: t('issue.micMac'), action: t('common.openSettings') }
          : { text: t('issue.micWindows'), action: t('common.openSettings') };
      case 'firewall':
        return { text: t('issue.firewall'), action: t('issue.allowFirewall') };
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
        {#if issue === 'localNetwork' && localNet.ask !== 'done'}<button class="btn" onclick={actions.askLocalNetwork} disabled={localNet.ask === 'asking'} title={t('network.askAgainTitle')}>{localNet.ask === 'asking' ? t('network.askingShort') : t('network.askAgain')}</button>{/if}
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
