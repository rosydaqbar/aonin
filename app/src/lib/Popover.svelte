<script lang="ts">
  import { Mic, MicOff, Monitor, Power } from '@lucide/svelte';
  import ClientRow from './ClientRow.svelte';
  import StatusMark from './StatusMark.svelte';
  import { app, fmtMs, QUALITY, roleLine } from './state.svelte';
  import { actions } from './actions';

  let dot = $derived(
    app.hosting ? 'var(--orange)'
    : app.link?.status === 'connected' ? 'var(--green)'
    : 'var(--strip-ink-2)'
  );
  let idle = $derived(!app.gate && !app.hosting && !app.link);
</script>

<div class="pop">
  <header class="strip">
    <span class="me" title={app.me.name}>{app.me.name}</span>
    <span class="role"><i style:background={dot}></i>{roleLine(app)}</span>
  </header>

  {#if app.error}
    <p class="alert" role="alert">{app.error}</p>
  {/if}

  {#if app.outputLost}
    <p class="alert" role="status">Output device disconnected. Reconnecting when it's back…</p>
  {/if}

  {#if app.gate === 'permission'}
    <section class="solo">
      <p>Aonin needs permission to capture this Mac's audio.</p>
      <button class="btn primary" onclick={actions.grantPermission}>allow audio capture</button>
    </section>
  {:else if app.gate === 'firewall'}
    <section class="solo">
      <p>Windows Firewall is stopping other devices from reaching this PC.</p>
      <button class="btn primary" onclick={actions.allowFirewall}>allow in firewall</button>
    </section>
  {:else if idle && app.hosts.length === 0}
    <section class="solo">
      {#if app.searching}
        <p class="searching"><StatusMark mark="connecting" /> Looking for hosts on this network…</p>
      {:else}
        <p>No host on this network yet.</p>
        <button class="btn primary" onclick={actions.becomeHost}>become host</button>
      {/if}
    </section>
  {/if}

  {#if app.link}
    <section class="block">
      <div class="head">
        <span class="label">{app.link.status === 'connected' ? 'sending to' : 'reconnecting to'}</span>
        {#if app.link.status === 'connected'}<span class="label num">{fmtMs(app.link.latencyMs)}</span>{/if}
      </div>
      <div class="host">
        <StatusMark mark={app.link.status === 'connected' ? 'live' : 'connecting'} />
        <span class="name">{app.link.host.name}</span>
        <button class="btn quiet" onclick={actions.disconnect}>disconnect</button>
      </div>
      <div class="chips" role="group" aria-label="Streams">
        <span class="label">send</span>
        <span class="set">
          <button class="chip" aria-pressed={app.link.send.system} onclick={() => actions.setSend('system', !app.link!.send.system)}><Monitor size={13} strokeWidth={1.75} />system audio</button>
          <button class="chip" aria-pressed={app.link.send.mic} onclick={() => actions.setSend('mic', !app.link!.send.mic)} aria-label={app.link.send.mic ? 'Turn microphone off' : 'Turn microphone on'}>{#if app.link.send.mic}<Mic size={13} strokeWidth={1.75} />mic on{:else}<MicOff size={13} strokeWidth={1.75} />mic off{/if}</button>
        </span>
        <span class="label">hear</span>
        <span class="set">
          <button class="chip" aria-pressed={app.link.hear.system} onclick={() => actions.setHear('system', !app.link!.hear.system)}><Monitor size={13} strokeWidth={1.75} />host audio</button>
          <button class="chip" aria-pressed={app.link.hear.mic} onclick={() => actions.setHear('mic', !app.link!.hear.mic)}><Mic size={13} strokeWidth={1.75} />host mic</button>
        </span>
      </div>
    </section>
  {/if}

  {#if app.hosting}
    <section class="block">
      <div class="head">
        <span class="label">clients</span>
        <span class="label" title="Output">to {app.hosting.output}</span>
      </div>
      {#each app.hosting.clients as row (row.id)}
        <ClientRow {row} />
      {:else}
        <p class="wait">Waiting for clients. Open Aonin on another device on this network.</p>
      {/each}
    </section>
  {/if}

  {#if idle && app.hosts.length > 0}
    <section class="block">
      <div class="head"><span class="label">hosts on this network</span></div>
      {#each app.hosts as h (h.id)}
        <div class="hostrow" class:blocked={h.blocked}>
          <StatusMark mark={h.blocked ? 'error' : 'live'} />
          <div class="who">
            <span class="name">{h.name}</span>
            <span class="label">{h.blocked ?? `${h.clients} ${h.clients === 1 ? 'client' : 'clients'}`}</span>
          </div>
          <button class="btn" disabled={!!h.blocked} onclick={() => actions.connect(h.id)}>
            {h.blocked ? "can't join" : 'connect'}
          </button>
        </div>
      {/each}
      <button class="btn quiet also" onclick={actions.becomeHost}>or become host instead</button>
    </section>
  {/if}

  <footer>
    <button class="btn quiet" onclick={() => actions.openWindow('advanced')}>{QUALITY[app.quality].label.toLowerCase()} · {QUALITY[app.quality].detail.split(',')[0]}</button>
    <span class="spacer"></span>
    <button class="btn quiet" onclick={() => actions.openWindow('devices')}>open aonin</button>
    <button class="icon" onclick={actions.quit} aria-label="Quit Aonin"><Power size={14} strokeWidth={1.75} /></button>
  </footer>
</div>

<style>
  .pop {
    width: 360px;
    background: var(--housing);
    border-radius: var(--radius);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .strip {
    background: var(--strip);
    color: var(--strip-ink);
    height: 48px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .me { font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .role {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: var(--size-s);
    letter-spacing: 0.04em;
    color: var(--strip-ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .role i { width: 8px; height: 8px; border-radius: 50%; flex: none; }
  .alert { margin: 12px 16px 0; padding: 9px 12px; border-radius: 8px; background: var(--red-wash); color: var(--ink); }
  .solo { padding: 28px 16px 26px; display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }
  .solo p { font-weight: 600; }
  .searching { display: flex; align-items: center; gap: 10px; font-weight: 400 !important; color: var(--ink-2); }
  .block { padding: 14px 16px 4px; }
  .block + .block { padding-top: 18px; }
  .head { display: flex; justify-content: space-between; gap: 12px; margin-bottom: 2px; }
  .head .label:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .host { display: flex; align-items: center; gap: 10px; min-height: var(--row); }
  .host .name { font-weight: 600; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chips { display: grid; grid-template-columns: 34px 1fr; align-items: center; gap: 8px 10px; padding: 2px 0 12px; border-bottom: 1px solid var(--line); }
  .set { display: flex; gap: 6px; }
  .chip {
    height: 26px;
    padding: 0 10px;
    border-radius: 13px;
    border: 1px solid var(--line-strong);
    background: var(--raised);
    font-size: var(--size-s);
    letter-spacing: 0.04em;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--ink-2);
  }
  .chip:disabled { opacity: 0.45; }
  .chip[aria-pressed='true'] { background: var(--strip); border-color: var(--strip); color: var(--strip-ink); }
  .wait { padding: 12px 0 14px; color: var(--ink-2); border-bottom: 1px solid var(--line); }
  .hostrow {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    min-height: 50px;
    border-bottom: 1px solid var(--line);
  }
  .who { display: flex; flex-direction: column; min-width: 0; }
  .who .name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .blocked .label { color: var(--red); }
  .also { margin: 8px 0 6px -14px; }
  footer {
    margin-top: 10px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 8px 10px;
    background: var(--recess);
    border-top: 1px solid var(--line);
  }
  .spacer { flex: 1; }
  .icon {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
    color: var(--ink-2);
  }
  .icon:hover { background: var(--housing); color: var(--ink); }
</style>
