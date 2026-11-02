<script lang="ts">
  import { AudioLines, Copy, Download, Globe, Info, Lock, LogIn, Mic, Monitor, MonitorSmartphone, Network, Power, RefreshCw, SlidersHorizontal, VolumeX, Wifi } from '@lucide/svelte';
  import ClientRow from './ClientRow.svelte';
  import Confirm from './Confirm.svelte';
  import Issues from './Issues.svelte';
  import JoinForm from './JoinForm.svelte';
  import ListenForm from './ListenForm.svelte';
  import Modal from './Modal.svelte';
  import StatusMark from './StatusMark.svelte';
  import Switch from './Switch.svelte';
  import { actions, type Section } from './actions';
  import { app, fmtCode, fmtMs, newCode, QUALITY, roleLine, type Quality } from './state.svelte';

  const sections: Section[] = ['devices', 'audio', 'network', 'advanced', 'about'];
  const sectionIcons = { devices: MonitorSmartphone, audio: AudioLines, network: Network, advanced: SlidersHorizontal, about: Info };
  const REPO = 'https://github.com/rosydaqbar/aonin';

  // About → Check for updates: idle, checking, or the outcome of the last check.
  let check = $state<'idle' | 'checking' | 'latest' | 'failed'>('idle');
  async function checkNow() {
    check = 'checking';
    try {
      check = (await actions.checkUpdate()) ? 'idle' : 'latest';
    } catch {
      check = 'failed';
    }
  }
  let section = $state<Section>(readHash());
  /** The Become listener or Join form, shown as a dialog. */
  let form = $state<'listen' | 'join' | null>(null);
  $effect(() => {
    if (form === 'join' && app.link) form = null;
  });
  /** Turning private drops audio senders without the code: ask first. */
  let confirmPrivate = $state(false);
  function makePrivate() {
    if (app.hosting?.clients.length) confirmPrivate = true;
    else actions.setPrivate(newCode());
  }
  let copied = $state(false);
  async function copyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {}
  }
  /** The audio sender the user asked to make the listener, awaiting confirmation. */
  let makeTarget = $state<{ id: string; name: string } | null>(null);

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
    <span class="brand">Aonin</span>
    <span class="me">{app.hosting?.name ?? app.me.name} · {app.me.os === 'macos' ? 'macOS' : 'Windows'}</span>
    <span class="role"><i style:background={dot}></i>{roleLine(app)}</span>
  </header>

  {#if app.hosting?.code}
    {@const code = app.hosting.code}
    <div class="vis">
      <span class="icon" aria-hidden="true"><Lock size={16} strokeWidth={1.75} /></span>
      <span class="text">
        <span class="title">Private</span>
        <span class="label">Only devices with the code can join. Audio senders type it under Join.</span>
      </span>
      <span class="bigcode code" aria-label="Code {code.split('').join(' ')}">{fmtCode(code)}</span>
      <button class="btn" onclick={() => copyCode(code)}><Copy size={14} strokeWidth={1.75} />{copied ? 'Copied' : 'Copy code'}</button>
      <button class="btn" onclick={() => actions.setPrivate(null)}><Globe size={14} strokeWidth={1.75} />Make public</button>
    </div>
  {:else if app.hosting}
    <div class="vis">
      <span class="icon" aria-hidden="true"><Globe size={16} strokeWidth={1.75} /></span>
      <span class="text">
        <span class="title">Visible to public</span>
        <span class="label">Anyone on this network can join.</span>
      </span>
      <button class="btn" onclick={makePrivate}><Lock size={14} strokeWidth={1.75} />Make private</button>
    </div>
  {/if}

  <div class="body">
    <nav aria-label="Sections">
      {#each sections as s}
        {@const Icon = sectionIcons[s]}
        <a href="#{s}" aria-current={section === s ? 'page' : undefined}><Icon size={15} strokeWidth={1.75} aria-hidden="true" />{s[0].toUpperCase() + s.slice(1)}</a>
      {/each}
      <button class="quit" onclick={actions.quit}><Power size={14} strokeWidth={1.75} />Quit Aonin</button>
    </nav>

    <main>
      <Issues />
      {#if app.error}
        <p class="alert" role="alert">{app.error}</p>
      {/if}
      {#if app.outputLost}
        <p class="alert" role="status">The output device isn't playing. It may be unplugged, or another app may be holding it. Aonin resumes as soon as it plays again.</p>
      {/if}

      {#if section === 'devices'}
        {#if app.hosting}
          <div class="titlebar">
            <h2>Audio senders</h2>
            <span class="label">Playing to {app.hosting.output}</span>
            <button class="btn" onclick={actions.stopHosting}>Stop listening</button>
          </div>
          {#if app.hosting.clients.length}
            <div class="cols label"><span></span><span>Device</span><span>Level</span><span class="r">Latency</span><span>Volume</span><span></span><span></span></div>
            {#each app.hosting.clients as row (row.id)}
              <ClientRow {row} wide lost={app.outputLost} onmake={app.handingOver || app.hosting.code ? undefined : () => (makeTarget = { id: row.id, name: row.name })} />
            {/each}
          {:else}
            <p class="empty">{app.hosting.code ? "Waiting for audio senders. On another device, open Aonin, choose Join and type the code above." : "Waiting for audio senders. Open Aonin on another device on this network and it will appear here."}</p>
          {/if}
        {/if}

        {#if app.link}
          <div class="titlebar">
            <h2>Connection</h2>
            {#if !app.hosting}<button class="btn" onclick={() => (form = 'listen')}>Become listener</button>{/if}
          </div>
          <div class="panel">
            <div class="linkhead">
              <StatusMark mark={app.link.status === 'connected' ? 'live' : 'connecting'} />
              <span class="name">{app.link.host.name}</span>
              <span class="label num">{app.link.status === 'connected' ? fmtMs(app.link.latencyMs) : 'Reconnecting…'}</span>
              <button class="btn" onclick={actions.disconnect}>Disconnect</button>
            </div>
          </div>

          <div class="titlebar">
            <h2>Send to {app.link.host.name}</h2>
            <span class="label">From this {app.me.os === 'macos' ? 'Mac' : 'PC'}, played on {app.link.host.name}</span>
          </div>
          <Switch icon={Monitor} label="System audio" hint="Everything this device plays" checked={app.link.send.system} onchange={(on) => actions.setSend('system', on)} />
          <Switch
            icon={VolumeX}
            label="Mute speakers while sending"
            hint={app.me.os === 'macos'
              ? 'This Mac stays quiet while its audio plays on the listener. What Aonin plays still comes through.'
              : 'This PC stays quiet while its audio plays on the listener, including what Aonin plays.'}
            checked={app.options.muteSpeakers}
            disabled={!app.link.send.system}
            onchange={(on) => actions.setMuteSpeakers(on)}
          />
          <Switch icon={Mic} label="Microphone" hint={app.devices.mic || app.devices.defaultMic} checked={app.link.send.mic} onchange={(on) => actions.setSend('mic', on)} />

          <div class="titlebar">
            <h2>Hear from {app.link.host.name}</h2>
            <span class="label">From {app.link.host.name}, played on this {app.me.os === 'macos' ? 'Mac' : 'PC'}</span>
          </div>
          <Switch icon={Monitor} label="System audio" hint="What {app.link.host.name}'s apps play, without the mix" checked={app.link.hear.system} onchange={(on) => actions.setHear('system', on)} />
          <Switch icon={Mic} label="Microphone" hint="{app.link.host.name}'s microphone" checked={app.link.hear.mic} onchange={(on) => actions.setHear('mic', on)} />
        {/if}

        {#if !app.hosting && !app.link}
          <div class="titlebar">
            <h2>Listeners on this network</h2>
            {#if app.hosts.length}
              <span class="pair">
                <button class="btn" onclick={() => (form = 'join')}>Join with code</button>
                <button class="btn" onclick={() => (form = 'listen')}>Become listener</button>
              </span>
            {/if}
          </div>
          {#if app.hosts.length === 0}
            <div class="solo">
              <p>No listener on this network yet.</p>
              <span class="pair">
                <button class="btn primary" onclick={() => (form = 'listen')}>Become listener</button>
                <button class="btn" onclick={() => (form = 'join')}>Join</button>
              </span>
              <p class="label">Joining a private listener? Use Join and type its code.</p>
            </div>
          {/if}
        {/if}

        {#if (app.hosting || !app.link) && others.length}
          {#if app.hosting}<div class="titlebar"><h2>Other listeners</h2></div>{/if}
          {#each others as h (h.id)}
            <div class="hostrow">
              <StatusMark mark={h.blocked ? 'error' : 'live'} />
              <div class="who">
                <span class="name">{h.name}</span>
                <span class="label" class:bad={h.blocked}>{h.blocked ?? `Listening · ${h.clients} ${h.clients === 1 ? 'audio sender' : 'audio senders'}`}</span>
              </div>
              {#if !h.blocked}<button class="btn" onclick={() => actions.connect(h.id)}>Connect</button>{/if}
            </div>
          {/each}
        {/if}
      {:else if section === 'audio'}
        <div class="titlebar"><h2>Audio devices</h2></div>
        <div class="form">
          <label><span>Play everything on</span>
            <select value={app.devices.output} onchange={(e) => actions.setDevice('output', e.currentTarget.value)}>
              <option value="">System default ({app.devices.defaultOutput || 'none'})</option>
              {#each app.devices.outputs as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">Where the mix comes out when this device is the listener. Follows the system default unless you pick one.</span></label>
          <label><span>System audio from</span>
            <select value={app.devices.systemSource} onchange={(e) => actions.setDevice('systemSource', e.currentTarget.value)}>
              <option value="">System default ({app.devices.defaultOutput || 'none'})</option>
              {#each app.devices.outputs as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">The output whose sound is sent to the listener.</span></label>
          <label><span>Microphone</span>
            <select value={app.devices.mic} onchange={(e) => actions.setDevice('mic', e.currentTarget.value)}>
              <option value="">System default ({app.devices.defaultMic || 'none'})</option>
              {#each app.devices.mics as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">Used when “Send microphone” is on.</span></label>
        </div>
      {:else if section === 'network'}
        <div class="titlebar"><h2>Connection health</h2></div>
        {#if app.stats.length}
          <table>
            <thead><tr class="label"><th></th><th>Device</th><th class="r">Latency</th><th class="r">Repaired</th><th class="r">Covered gaps</th><th class="r">Dropouts</th></tr></thead>
            <tbody>
              {#each app.stats as s}
                <tr>
                  <td class="dot"><StatusMark mark={s.dropouts > 0 ? 'error' : s.concealed > 0 ? 'degraded' : 'live'} /></td>
                  <td>{s.name}</td>
                  <td class="num r">{fmtMs(s.bufferMs)} <span class="label">target {fmtMs(s.targetMs)}</span></td>
                  <td class="num r">{s.recovered}</td>
                  <td class="num r" class:warnc={s.concealed > 0}>{s.concealed}</td>
                  <td class="num r" class:badc={s.dropouts > 0}>{s.dropouts}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <dl class="legend">
            <dt>Repaired</dt><dd>Packets Wi-Fi lost that Aonin got back in time. Nothing was lost.</dd>
            <dt>Covered gaps</dt><dd>Packets that never arrived, smoothed over. Above zero means some audio was filled in.</dd>
            <dt>Dropouts</dt><dd>The buffer ran dry and sound paused. Aonin raises the buffer after each one.</dd>
          </dl>
        {:else}
          <p class="empty">Nothing is streaming right now.</p>
        {/if}
        {#if app.me.os === 'macos'}
          <div class="titlebar"><h2>Local network access</h2></div>
          <div class="update">
            <span class="icon" aria-hidden="true"><Network size={16} strokeWidth={1.75} /></span>
            <span class="text">
              <span>{app.issues.includes('localNetwork') ? 'macOS is blocking Aonin from this network' : 'Aonin can reach this network'}</span>
              <span class="label">macOS asks once. If it never asked, or Aonin is missing from Privacy &amp; Security › Local Network, ask again. Then switch Aonin on there.</span>
            </span>
            <button class="btn" onclick={actions.askLocalNetwork}>Ask again</button>
            <button class="btn" onclick={() => actions.fixIssue('localNetwork')}>Open settings</button>
          </div>
          <div class="titlebar"><h2>Wi-Fi</h2></div>
          <Switch icon={Wifi} label="Keep Wi-Fi steady" hint="Turns off AirDrop and Handoff scanning, which makes Mac Wi-Fi stall every few seconds. Asks for your password; back on when you switch this off or restart." checked={app.options.quietWifi} onchange={(on) => actions.setQuietWifi(on)} />
        {/if}
      {:else if section === 'about'}
        <div class="about">
          <img src="/favicon.png" alt="" width="64" height="64" />
          <div>
            <h2>Aonin</h2>
            <p class="label">Version {app.version}</p>
          </div>
        </div>
        <p class="overview">Aonin brings the audio of the computers on your local network into one place, in real time and at the highest quality over Wi-Fi. Any computer can be the listener; the others send their audio to it and can hear the listener back. No account and no server: everything stays on your network.</p>

        <div class="titlebar"><h2>Updates</h2></div>
        <div class="update">
          <span class="icon" aria-hidden="true"><Download size={16} strokeWidth={1.75} /></span>
          <span class="text">
            {#if app.update}
              <span>Aonin {app.update} is ready</span>
              <span class="label">Restarting takes a few seconds. Aonin reconnects after.</span>
            {:else if check === 'checking'}
              <span>Checking for updates…</span>
              <span class="label">A newer version downloads in the background.</span>
            {:else if check === 'latest'}
              <span>Aonin {app.version} is the latest version</span>
              <span class="label">Aonin also checks by itself every few hours.</span>
            {:else if check === 'failed'}
              <span>Couldn’t check for updates</span>
              <span class="label bad">GitHub couldn’t be reached. Check the internet connection and try again.</span>
            {:else}
              <span>Aonin {app.version}</span>
              <span class="label">Checks for updates by itself and downloads them in the background.</span>
            {/if}
          </span>
          {#if app.update}
            <button class="btn primary" onclick={actions.installUpdate}>Restart to update</button>
          {:else}
            <button class="btn" onclick={checkNow} disabled={check === 'checking'}>Check for updates</button>
          {/if}
        </div>

        <div class="titlebar"><h2>License</h2></div>
        <p class="legal">© 2026 Rosyd Aqbar. Aonin is free software under the GNU General Public License, version 3 or later. It comes with no warranty.</p>
        <p class="links">
          <button class="link" onclick={() => actions.openUrl(REPO)}>Source code on GitHub</button>
          <button class="link" onclick={() => actions.openUrl(REPO + '/blob/main/LICENSE')}>Read the license</button>
        </p>
      {:else}
        <div class="titlebar"><h2>Quality</h2><span class="label">For audio this device sends</span></div>
        <div class="radios" role="radiogroup" aria-label="Quality">
          {#each Object.entries(QUALITY) as [key, q]}
            <label class="radio">
              <input type="radio" name="quality" value={key} checked={app.quality === key} onchange={() => actions.setQuality(key as Quality)} />
              <span class="dotbox" aria-hidden="true"></span>
              <span class="qname">{q.label}</span>
              <span class="label">{q.detail}</span>
              <span class="label num r">{q.rate}</span>
            </label>
          {/each}
        </div>
        <div class="titlebar"><h2>Startup</h2></div>
        <Switch icon={RefreshCw} label="Reconnect automatically" hint="Send to the last listener as soon as Aonin opens" checked={app.options.reconnect} onchange={(on) => actions.setReconnect(on)} />
        <Switch icon={LogIn} label="Open at login" hint="Start Aonin in the background when you sign in" checked={app.options.startAtLogin} onchange={(on) => actions.setStartAtLogin(on)} />
      {/if}
    </main>
  </div>
</div>

{#if form}
  <Modal label={form === 'listen' ? 'Become the listener' : 'Join a private listener'} onclose={() => { if (form === 'join') actions.cancelJoin(); form = null; }}>
    {#if form === 'listen'}
      <ListenForm oncancel={() => (form = null)} ondone={() => (form = null)} />
    {:else}
      <JoinForm oncancel={() => (form = null)} />
    {/if}
  </Modal>
{/if}
{#if confirmPrivate}
  <Confirm
    title="Make this listener private?"
    body="Audio senders will need the code to stay connected. They type it under Join."
    action="Make private"
    onconfirm={() => actions.setPrivate(newCode())}
    onclose={() => (confirmPrivate = false)}
  />
{/if}
{#if makeTarget}
  {@const t = makeTarget}
  <Confirm
    title="Make {t.name} the listener?"
    body="Everyone's audio will play on {t.name}, and this {app.me.os === 'macos' ? 'Mac' : 'PC'} will send its audio there."
    action="Make listener"
    onconfirm={() => actions.handOver(t.id)}
    onclose={() => (makeTarget = null)}
  />
{/if}

<style>
  .win { height: 100vh; display: grid; grid-template-rows: 52px auto 1fr; background: var(--housing); }
  .win > .body:nth-child(2) { grid-row: 2 / 4; }
  .vis { display: flex; align-items: center; gap: 12px; padding: 10px 20px; border-bottom: 1px solid var(--line); background: var(--recess); }
  .vis .icon { display: grid; place-items: center; width: 28px; height: 28px; border-radius: 7px; background: var(--raised); color: var(--ink-2); flex: none; }
  .vis .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .vis .title { font-weight: 700; }
  .vis .btn { display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .bigcode { font-size: 18px; padding: 0 6px; user-select: text; cursor: text; }
  .pair { display: flex; gap: 8px; }
  .titlebar h2 + .pair { margin-left: auto; }
  .solo p.label { font-weight: 400; }
  .strip { background: var(--strip); color: var(--strip-ink); display: flex; align-items: center; gap: 14px; padding: 0 20px; }
  .brand { font-weight: 700; font-size: var(--size); letter-spacing: 0.02em; }
  .me { font-size: var(--size-s); letter-spacing: 0.04em; color: var(--strip-ink-2); }
  .role { margin-left: auto; display: flex; align-items: center; gap: 8px; font-size: var(--size-s); letter-spacing: 0.04em; color: var(--strip-ink-2); }
  .role i { width: 8px; height: 8px; border-radius: 50%; }
  .body { display: grid; grid-template-columns: 172px 1fr; min-height: 0; }
  nav { border-right: 1px solid var(--line); padding: 16px 10px; display: flex; flex-direction: column; gap: 2px; }
  nav a { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 6px; color: var(--ink); text-decoration: none; }
  nav a :global(svg) { color: var(--ink-2); flex: none; }
  nav a[aria-current='page'] :global(svg) { color: var(--ink); }
  nav a:hover { color: var(--ink); background: color-mix(in srgb, var(--recess) 50%, transparent); }
  nav a[aria-current='page'] { background: var(--raised); font-weight: 700; box-shadow: 0 1px 2px rgb(0 0 0 / 0.08); }
  .quit {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--ink-2);
    text-align: left;
  }
  .quit:hover { background: var(--recess); color: var(--ink); }
  main { padding: 8px 28px 28px; overflow-y: auto; }
  h2 { font-size: var(--size); font-weight: 700; }
  .titlebar { display: flex; align-items: center; gap: 14px; margin: 22px 0 8px; min-height: 28px; }
  .titlebar .label { margin-left: auto; }
  .titlebar .label + .btn { margin-left: 0; }
  .titlebar h2 + .btn { margin-left: auto; }
  .cols { display: grid; grid-template-columns: 8px minmax(120px, 1fr) 105px 64px 100px 28px 124px; gap: 12px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
  .r { text-align: right; }
  .empty { color: var(--ink-2); padding: 14px 0; }
  .alert { margin-top: 16px; padding: 10px 12px; border-radius: 8px; background: var(--red-wash); }
  .panel { border-top: 1px solid var(--line); }
  .linkhead { display: flex; align-items: center; gap: 12px; min-height: 52px; border-bottom: 1px solid var(--line); }
  .linkhead .name { font-weight: 700; flex: 1; }
  .about { display: flex; align-items: center; gap: 16px; margin: 22px 0 14px; }
  .about img { border-radius: 14px; }
  .about h2 { font-size: 20px; }
  .overview { max-width: 62ch; line-height: 1.55; }
  .legal { max-width: 62ch; line-height: 1.55; color: var(--ink-2); padding-top: 4px; }
  .links { display: flex; gap: 18px; margin-top: 10px; }
  .link { background: none; border: 0; padding: 0; font: inherit; color: var(--ink); text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }
  .update { display: flex; align-items: center; gap: 12px; min-height: 52px; border-bottom: 1px solid var(--line); }
  .update .icon { display: grid; place-items: center; width: 28px; height: 28px; border-radius: 7px; background: var(--recess); color: var(--ink-2); flex: none; }
  .update .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
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
  td:nth-child(2) { font-weight: 600; }
  td.dot, th:first-child { width: 20px; }
  th.r, td.r { text-align: right; }
  .warnc { color: var(--yellow-ink); font-weight: 600; }
  .badc { color: var(--red); font-weight: 600; }
  .legend { display: grid; grid-template-columns: 110px 1fr; gap: 6px 16px; margin-top: 16px; max-width: 620px; color: var(--ink-2); }
  .legend dt { color: var(--ink); font-weight: 600; }
  .radios { border-top: 1px solid var(--line); }
  .radio { display: grid; grid-template-columns: 18px 84px 1fr auto; gap: 12px; align-items: center; min-height: 46px; border-bottom: 1px solid var(--line); }
  .radio input { position: absolute; opacity: 0; width: 1px; height: 1px; }
  .dotbox { width: 16px; height: 16px; border-radius: 50%; border: 1px solid var(--line-strong); background: var(--raised); }
  .radio input:checked + .dotbox { border: 5px solid var(--hole-fill); }
  .radio input:focus-visible + .dotbox { outline: 2px solid var(--focus); outline-offset: 2px; }
  .qname { font-weight: 600; }
</style>
