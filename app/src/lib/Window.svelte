<script lang="ts">
  import ClientRow from './ClientRow.svelte';
  import StatusMark from './StatusMark.svelte';
  import Switch from './Switch.svelte';
  import { actions, type Section } from './actions';
  import { app, fmtMs, QUALITY, roleLine, type Quality } from './state.svelte';

  const sections: Section[] = ['devices', 'audio', 'network', 'advanced'];
  let section = $state<Section>(readHash());

  function readHash(): Section {
    const h = (typeof location === 'undefined' ? '' : location.hash.slice(1)) as Section;
    return sections.includes(h) ? h : 'devices';
  }

  let dot = $derived(app.hosting ? 'var(--orange)' : app.link?.status === 'connected' ? 'var(--green)' : 'var(--strip-ink-2)');
  let others = $derived(app.hosts.filter((h) => h.id !== app.link?.host.id));
</script>

<svelte:window onhashchange={() => (section = readHash())} />

<div class="win">
  <header class="strip">
    <span class="brand">aonin</span>
    <span class="me">{app.me.name} · {app.me.os === 'macos' ? 'macOS' : 'Windows'}</span>
    <span class="role"><i style:background={dot}></i>{roleLine(app)}</span>
  </header>

  <div class="body">
    <nav aria-label="Sections">
      {#each sections as s}
        <a href="#{s}" aria-current={section === s ? 'page' : undefined}>{s}</a>
      {/each}
    </nav>

    <main>
      {#if app.error}
        <p class="alert" role="alert">{app.error}</p>
      {/if}
      {#if app.outputLost}
        <p class="alert" role="status">Output device disconnected. Aonin will resume as soon as it's back.</p>
      {/if}

      {#if section === 'devices'}
        {#if app.hosting}
          <div class="titlebar">
            <h2>Clients</h2>
            <span class="label">playing to {app.hosting.output}</span>
            <button class="btn" onclick={actions.stopHosting}>stop hosting</button>
          </div>
          {#if app.hosting.clients.length}
            <div class="cols label"><span></span><span>device</span><span>level</span><span class="r">latency</span><span>volume</span><span></span></div>
            {#each app.hosting.clients as row (row.id)}<ClientRow {row} wide />{/each}
          {:else}
            <p class="empty">Waiting for clients. Open Aonin on another device on this network and it will appear here.</p>
          {/if}
        {/if}

        {#if app.link}
          <div class="titlebar"><h2>Connection</h2></div>
          <div class="panel">
            <div class="linkhead">
              <StatusMark mark={app.link.status === 'connected' ? 'live' : 'connecting'} />
              <span class="name">{app.link.host.name}</span>
              <span class="label num">{app.link.status === 'connected' ? fmtMs(app.link.latencyMs) : 'reconnecting…'}</span>
              <button class="btn" onclick={actions.disconnect}>disconnect</button>
            </div>
            <div class="grid2">
              <Switch label="Send system audio" hint="everything this device plays" checked={app.link.send.system} onchange={(on) => actions.setSend('system', on)} />
              <Switch label="Hear host audio" hint="coming soon" disabled />
              <Switch label="Send microphone" hint={app.devices.mic} checked={app.link.send.mic} onchange={(on) => actions.setSend('mic', on)} />
              <Switch label="Hear host mic" hint="coming soon" disabled />
            </div>
          </div>
        {/if}

        {#if !app.hosting && !app.link}
          <div class="titlebar">
            <h2>Hosts on this network</h2>
            {#if app.hosts.length}<button class="btn" onclick={actions.becomeHost}>become host</button>{/if}
          </div>
          {#if app.hosts.length === 0}
            <div class="solo">
              <p>{app.searching ? 'Looking for hosts on this network…' : 'No host on this network yet.'}</p>
              {#if !app.searching}<button class="btn primary" onclick={actions.becomeHost}>become host</button>{/if}
            </div>
          {/if}
        {/if}

        {#if (app.hosting || !app.link) && others.length}
          {#if app.hosting}<div class="titlebar"><h2>Other hosts</h2></div>{/if}
          {#each others as h (h.id)}
            <div class="hostrow">
              <StatusMark mark={h.blocked ? 'error' : 'live'} />
              <div class="who">
                <span class="name">{h.name}</span>
                <span class="label" class:bad={h.blocked}>{h.blocked ?? `hosting · ${h.clients} ${h.clients === 1 ? 'client' : 'clients'}`}</span>
              </div>
              <button class="btn" disabled={!!h.blocked} onclick={() => actions.connect(h.id)}>{h.blocked ? "can't join" : 'connect'}</button>
            </div>
          {/each}
        {/if}
      {:else if section === 'audio'}
        <div class="titlebar"><h2>Audio devices</h2></div>
        <div class="form">
          <label><span>Play everything on</span><select bind:value={app.devices.output}>{#each app.devices.outputs as d}<option>{d}</option>{/each}</select><span class="label">where the mix comes out when this device is host</span></label>
          <label><span>System audio from</span><select bind:value={app.devices.systemSource}>{#each app.devices.outputs as d}<option>{d}</option>{/each}</select><span class="label">the output whose sound is sent to the host</span></label>
          <label><span>Microphone</span><select bind:value={app.devices.mic}>{#each app.devices.mics as d}<option>{d}</option>{/each}</select><span class="label">used when “send microphone” is on</span></label>
        </div>
      {:else if section === 'network'}
        <div class="titlebar"><h2>Connection health</h2></div>
        {#if app.stats.length}
          <table>
            <thead><tr class="label"><th>device</th><th>buffer</th><th>repaired</th><th>covered gaps</th><th>dropouts</th></tr></thead>
            <tbody>
              {#each app.stats as s}
                <tr><td>{s.name}</td><td class="num">{fmtMs(s.bufferMs)} <span class="label">of {fmtMs(s.targetMs)}</span></td><td class="num">{s.recovered}</td><td class="num">{s.concealed}</td><td class="num">{s.dropouts}</td></tr>
              {/each}
            </tbody>
          </table>
          <dl class="legend">
            <dt>Repaired</dt><dd>Packets Wi-Fi lost that Aonin got back in time. Nothing was lost.</dd>
            <dt>Covered gaps</dt><dd>Packets that never arrived, smoothed over. Above zero means a moment was not lossless.</dd>
            <dt>Dropouts</dt><dd>The buffer ran dry and sound paused. Aonin raises the buffer after each one.</dd>
          </dl>
        {:else}
          <p class="empty">Nothing is streaming right now.</p>
        {/if}
        {#if app.me.os === 'macos'}
          <div class="titlebar"><h2>Wi-Fi</h2></div>
          <Switch label="Keep Wi-Fi steady while streaming" hint="coming soon · pauses AirDrop and Handoff, which make Mac Wi-Fi stall every few seconds" disabled />
        {/if}
      {:else}
        <div class="titlebar"><h2>Quality</h2></div>
        <div class="radios" role="radiogroup" aria-label="Quality">
          {#each Object.entries(QUALITY) as [key, q]}
            <label class="radio">
              <input type="radio" name="quality" value={key} bind:group={app.quality} disabled={key !== 'lossless'} />
              <span class="dotbox" aria-hidden="true"></span>
              <span class="qname">{q.label}</span>
              <span class="label">{key === 'lossless' ? q.detail : `coming soon · ${q.detail}`}</span>
              <span class="label num r">{q.rate}</span>
            </label>
          {/each}
        </div>
        <div class="titlebar"><h2>Startup</h2></div>
        <Switch label="Reconnect automatically" hint="join the last host as soon as Aonin opens" checked={app.options.reconnect} onchange={(on) => actions.setReconnect(on)} />
        <Switch label="Open at login" hint="coming soon" disabled />
      {/if}
    </main>
  </div>
</div>

<style>
  .win { height: 100vh; display: grid; grid-template-rows: 52px 1fr; background: var(--housing); }
  .strip { background: var(--strip); color: var(--strip-ink); display: flex; align-items: center; gap: 14px; padding: 0 20px; }
  .brand { font-weight: 700; font-size: var(--size); letter-spacing: 0.02em; }
  .me { font-size: var(--size-s); letter-spacing: 0.04em; color: var(--strip-ink-2); }
  .role { margin-left: auto; display: flex; align-items: center; gap: 8px; font-size: var(--size-s); letter-spacing: 0.04em; color: var(--strip-ink-2); }
  .role i { width: 8px; height: 8px; border-radius: 50%; }
  .body { display: grid; grid-template-columns: 172px 1fr; min-height: 0; }
  nav { border-right: 1px solid var(--line); padding: 16px 10px; display: flex; flex-direction: column; gap: 2px; }
  nav a { padding: 8px 10px; border-radius: 6px; color: var(--ink); text-decoration: none; }
  nav a:hover { background: var(--recess); }
  nav a[aria-current='page'] { background: var(--recess); font-weight: 700; }
  main { padding: 8px 28px 28px; overflow-y: auto; }
  h2 { font-size: var(--size); font-weight: 700; text-transform: uppercase; letter-spacing: 0.07em; }
  .titlebar { display: flex; align-items: center; gap: 14px; margin: 22px 0 8px; min-height: 28px; }
  .titlebar .label { margin-left: auto; }
  .titlebar .label + .btn { margin-left: 0; }
  .titlebar h2 + .btn { margin-left: auto; }
  .cols { display: grid; grid-template-columns: 8px minmax(0, 1fr) 159px 64px 132px 64px; gap: 16px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
  .r { text-align: right; }
  .empty { color: var(--ink-2); padding: 14px 0; }
  .alert { margin-top: 16px; padding: 10px 12px; border-radius: 8px; background: var(--red-wash); }
  .panel { border-top: 1px solid var(--line); }
  .linkhead { display: flex; align-items: center; gap: 12px; min-height: 52px; border-bottom: 1px solid var(--line); }
  .linkhead .name { font-weight: 700; flex: 1; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; column-gap: 36px; }
  .solo { display: flex; flex-direction: column; align-items: flex-start; gap: 14px; padding: 18px 0; }
  .solo p { font-weight: 600; }
  .hostrow { display: grid; grid-template-columns: 8px minmax(0, 1fr) auto; gap: 16px; align-items: center; min-height: 52px; border-bottom: 1px solid var(--line); }
  .who { display: flex; flex-direction: column; min-width: 0; }
  .who .name { font-weight: 600; }
  .bad { color: var(--red); }
  .form { display: flex; flex-direction: column; gap: 18px; max-width: 520px; }
  .form label { display: grid; gap: 5px; }
  select {
    height: 32px;
    padding: 0 10px;
    border-radius: 8px;
    border: 1px solid var(--line-strong);
    background: var(--raised);
  }
  table { width: 100%; border-collapse: collapse; }
  th { text-align: left; font-weight: 400; padding: 0 0 6px; border-bottom: 1px solid var(--line); }
  td { padding: 0; height: 44px; border-bottom: 1px solid var(--line); }
  td:first-child { font-weight: 600; }
  .legend { display: grid; grid-template-columns: 110px 1fr; gap: 6px 16px; margin-top: 16px; max-width: 620px; color: var(--ink-2); }
  .legend dt { color: var(--ink); font-weight: 600; }
  .radios { border-top: 1px solid var(--line); }
  .radio { display: grid; grid-template-columns: 18px 84px 1fr auto; gap: 12px; align-items: center; min-height: 46px; border-bottom: 1px solid var(--line); }
  .radio input { position: absolute; opacity: 0; width: 1px; height: 1px; }
  .dotbox { width: 16px; height: 16px; border-radius: 50%; border: 1px solid var(--line-strong); background: var(--raised); }
  .radio:has(input:disabled) { opacity: 0.5; }
  .radio input:checked + .dotbox { border: 5px solid var(--hole-fill); }
  .radio input:focus-visible + .dotbox { outline: 2px solid var(--focus); outline-offset: 2px; }
  .qname { font-weight: 600; }
</style>
