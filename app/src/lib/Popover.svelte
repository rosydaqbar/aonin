<script lang="ts">
  import Issues from './Issues.svelte';
  import { ChevronDown, ChevronRight, Globe, Info, Lock, Mic, MicOff, Monitor, Power } from '@lucide/svelte';
  import ClientRow from './ClientRow.svelte';
  import Grille from './Grille.svelte';
  import StatusMark from './StatusMark.svelte';
  import { app, fmtMs, QUALITY, roleLine } from './state.svelte';
  import { actions } from './actions';

  let dot = $derived(
    app.hosting && app.outputLost ? 'var(--red)'
    : app.hosting ? 'var(--orange)'
    : app.link?.status === 'connected' ? 'var(--green)'
    : 'var(--strip-ink-2)'
  );
  let idle = $derived(!app.hosting && !app.link);
  // "Make private" isn't built yet: say so inline for a few seconds.
  let soon = $state(false);
  let soonTimer: ReturnType<typeof setTimeout> | undefined;
  function showSoon() {
    soon = true;
    clearTimeout(soonTimer);
    soonTimer = setTimeout(() => (soon = false), 4000);
  }

  // Send/Hear stay folded away until asked for; the choice is remembered here.
  let showStreams = $state(readShow());
  function readShow(): boolean {
    try {
      return localStorage.getItem('aonin.showStreams') === '1';
    } catch {
      return false;
    }
  }
  function toggleStreams() {
    showStreams = !showStreams;
    try {
      localStorage.setItem('aonin.showStreams', showStreams ? '1' : '0');
    } catch {}
  }
  let streamSummary = $derived.by(() => {
    const l = app.link;
    if (!l) return '';
    const send = [l.send.system && 'System audio', l.send.mic ? 'Mic on' : 'Mic off'].filter(Boolean).join(' · ');
    const hear = [l.hear.system && 'audio', l.hear.mic && 'mic'].filter(Boolean).join(' and ');
    return hear ? `${send} · Hearing their ${hear}` : send;
  });
</script>

<div class="pop">
  <header class="strip">
    <span class="me" title={app.me.name}>{app.me.name}</span>
    <span class="role"><i style:background={dot}></i>{roleLine(app)}</span>
    {#if app.hosting}
      <button class="stop" onclick={actions.stopHosting}>{app.link ? 'Stop listening' : 'Stop'}</button>
    {/if}
  </header>

  {#if app.hosting}
    <div class="vis">
      <Globe size={14} strokeWidth={1.75} aria-hidden="true" />
      <span>Visible to public</span>
      <span class="tip">
        <button class="info" aria-label="What visible to public means" aria-describedby="vis-tip"><Info size={13} strokeWidth={1.75} /></button>
        <span class="bubble" role="tooltip" id="vis-tip">Anyone on this network can join.</span>
      </span>
      <button class="btn quiet make" onclick={showSoon}><Lock size={13} strokeWidth={1.75} />Make private</button>
    </div>
    {#if soon}<p class="soon" role="status">Private mode is coming soon.</p>{/if}
  {/if}

  {#if app.error}
    <p class="alert" role="alert">{app.error}</p>
  {/if}

  {#if app.outputLost && app.hosting}
    <div class="alert lost" role="status">
      <p>{app.hosting.output} was disconnected. Playing again when it's back.</p>
      <button class="btn" onclick={() => actions.openWindow('audio')}>Choose output</button>
    </div>
  {/if}

  <Issues compact />

  {#if idle && app.hosts.length === 0}
    <section class="solo">
      <p>No listener on this network yet.</p>
      <button class="btn primary" onclick={actions.becomeHost}>Become listener</button>
    </section>
  {/if}

  {#if app.link}
    <section class="block">
      <div class="head">
        <span class="label">{app.link.status === 'connected' ? 'Sending to' : 'Reconnecting to'}</span>
        {#if app.link.status === 'connected'}<span class="label num">{fmtMs(app.link.latencyMs)}</span>{/if}
      </div>
      <div class="host">
        <StatusMark mark={app.link.status === 'connected' ? 'live' : 'connecting'} />
        <span class="name">{app.link.host.name}</span>
        <Grille level={app.link.sendLevel ?? 0} holes={8} muted={!app.link.send.system && !app.link.send.mic} />
        <button class="btn" onclick={actions.disconnect}>Disconnect</button>
      </div>
      <button class="disclose" aria-expanded={showStreams} aria-controls="streams" onclick={toggleStreams}>
        <span class="label">Streams</span>
        <span class="summary">{streamSummary}</span>
        <span class="chev" class:open={showStreams}><ChevronDown size={14} strokeWidth={1.75} /></span>
      </button>
      {#if showStreams}
      <div class="chips" id="streams" role="group" aria-label="Streams">
        <span class="label">Send</span>
        <span class="set">
          <button class="chip" aria-pressed={app.link.send.system} onclick={() => actions.setSend('system', !app.link!.send.system)}><Monitor size={13} strokeWidth={1.75} />System audio</button>
          <button class="chip" aria-pressed={app.link.send.mic} onclick={() => actions.setSend('mic', !app.link!.send.mic)} aria-label={app.link.send.mic ? 'Turn microphone off' : 'Turn microphone on'}>{#if app.link.send.mic}<Mic size={13} strokeWidth={1.75} />Mic on{:else}<MicOff size={13} strokeWidth={1.75} />Mic off{/if}</button>
        </span>
        <span class="label">Hear</span>
        <span class="set">
          <button class="chip" aria-pressed={app.link.hear.system} onclick={() => actions.setHear('system', !app.link!.hear.system)}><Monitor size={13} strokeWidth={1.75} />System audio</button>
          <button class="chip" aria-pressed={app.link.hear.mic} onclick={() => actions.setHear('mic', !app.link!.hear.mic)}><Mic size={13} strokeWidth={1.75} />Mic</button>
        </span>
      </div>
      {/if}
    </section>
  {/if}

  {#if app.hosting}
    <section class="block">
      <div class="head">
        <span class="label">Audio senders</span>
        <span class="label" title="Output">{app.outputLost ? 'No output' : `To ${app.hosting.output}`}</span>
      </div>
      {#each app.hosting.clients as row (row.id)}
        <ClientRow {row} lost={app.outputLost} />
      {:else}
        <p class="wait">Waiting for audio senders. Open Aonin on another device on this network.</p>
      {/each}
    </section>
  {/if}

  {#if idle && app.hosts.length > 0}
    <section class="block">
      <div class="head"><span class="label">Listeners on this network</span></div>
      {#each app.hosts as h (h.id)}
        <div class="hostrow" class:blocked={h.blocked}>
          <StatusMark mark={h.blocked ? 'error' : 'live'} />
          <div class="who">
            <span class="name">{h.name}</span>
            <span class="label">{h.blocked ?? `${h.clients} ${h.clients === 1 ? 'audio sender' : 'audio senders'}`}</span>
          </div>
          {#if !h.blocked}
            <button class="btn" onclick={() => actions.connect(h.id)}>Connect</button>
          {/if}
        </div>
      {/each}
      <button class="btn quiet also" onclick={actions.becomeHost}>Or become the listener instead</button>
    </section>
  {/if}

  <footer>
    <button class="btn quiet quality" onclick={() => actions.openWindow('advanced')}>{QUALITY[app.quality].label}{#if !app.update} · {QUALITY[app.quality].detail.split(',')[0]}{/if}<ChevronRight size={12} strokeWidth={1.75} /></button>
    <span class="spacer"></span>
    {#if app.update}<button class="btn primary" onclick={actions.installUpdate} title="Install Aonin {app.update} and restart">Update</button>{/if}
    <button class="btn quiet" onclick={() => actions.openWindow('devices')}>Open Aonin</button>
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
  .me { font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 0 1 auto; min-width: 64px; }
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
  .stop {
    flex: none;
    height: 24px;
    padding: 0 10px;
    border-radius: 12px;
    border: 1px solid var(--strip-ink-2);
    background: transparent;
    color: var(--strip-ink);
    font-size: var(--size-s);
    letter-spacing: 0.04em;
  }
  .stop:hover { background: var(--strip-ink); color: var(--strip); }
  .vis {
    display: flex;
    align-items: center;
    gap: 7px;
    min-height: 38px;
    padding: 0 6px 0 16px;
    border-bottom: 1px solid var(--line);
    font-size: var(--size-s);
    color: var(--ink-2);
  }
  .vis > :global(svg) { color: var(--ink-2); flex: none; }
  .vis > span:not(.tip) { color: var(--ink); font-weight: 600; }
  .tip { position: relative; display: inline-grid; }
  .info { display: grid; place-items: center; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--ink-2); }
  .info:hover, .info:focus-visible { color: var(--ink); background: var(--recess); }
  .bubble {
    position: absolute;
    top: calc(100% + 6px);
    left: -60px;
    z-index: 10;
    width: 236px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--strip);
    color: var(--strip-ink);
    font-size: var(--size-s);
    line-height: 1.45;
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.18);
    opacity: 0;
    visibility: hidden;
    transition: opacity 120ms ease;
    pointer-events: none;
  }
  .tip:hover .bubble, .info:focus-visible + .bubble { opacity: 1; visibility: visible; }
  @media (prefers-reduced-motion: reduce) { .bubble { transition: none; } }
  .soon { padding: 8px 16px; border-bottom: 1px solid var(--line); background: var(--recess); font-size: var(--size-s); }
  .make { margin-left: auto; display: inline-flex; align-items: center; gap: 6px; }
  .alert { margin: 12px 16px 0; padding: 9px 12px; border-radius: 8px; background: var(--red-wash); color: var(--ink); }
  .alert.lost { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; }
  .quality { color: var(--ink-2); }
  .quality:hover { color: var(--ink); }
  .solo { padding: 28px 16px 26px; display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }
  .solo p { font-weight: 600; }
  .block { padding: 14px 16px 4px; }
  .block + .block { padding-top: 18px; }
  .head { display: flex; justify-content: space-between; gap: 12px; margin-bottom: 2px; }
  .head .label:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .host { display: flex; align-items: center; gap: 10px; min-height: var(--row); }
  .host .name { font-weight: 600; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .disclose {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 36px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    text-align: left;
  }
  .disclose[aria-expanded='true'] { border-bottom-color: transparent; }
  .disclose .summary { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-2); font-size: var(--size-s); }
  .disclose:hover .summary { color: var(--ink); }
  .chev { display: grid; color: var(--ink-2); transition: transform 160ms cubic-bezier(0.16, 1, 0.3, 1); }
  .chev.open { transform: rotate(180deg); }
  @media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
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
    padding: 8px 2px;
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
