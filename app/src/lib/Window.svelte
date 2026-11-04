<script lang="ts">
  import { AudioLines, ArrowLeftRight, ChevronDown, Copy, Download, Globe, Info, Languages, Lock, LogIn, Mic, Monitor, MonitorSmartphone, Network, Power, RefreshCw, SlidersHorizontal, Sparkles, VolumeX, Wifi } from '@lucide/svelte';
  import ClientRow from './ClientRow.svelte';
  import EditionBadge from './EditionBadge.svelte';
  import Confirm from './Confirm.svelte';
  import Issues from './Issues.svelte';
  import JoinForm from './JoinForm.svelte';
  import ListenForm from './ListenForm.svelte';
  import Modal from './Modal.svelte';
  import ListenerNote from './ListenerNote.svelte';
  import ProFeatures from './ProFeatures.svelte';
  import ProModal from './ProModal.svelte';
  import StatusMark from './StatusMark.svelte';
  import Switch from './Switch.svelte';
  import { actions, type Section } from './actions';
  import { fmtDate, i18n, LANGUAGES, setLanguage, systemLang, t, tm, tn, type LangChoice } from './i18n/index.svelte';
  import { app, fmtCode, fmtMs, localNet, newCode, PRO_LINKS, proModal, QUALITY, roleLine, type Quality } from './state.svelte';

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
  /** Make private / Make listener when Pro isn't unlocked: the Unlock Pro dialog. */
  const getPro = () => actions.getPro();
  let section = $state<Section>(readHash());
  /** #pro (from the tray) opens the Unlock Pro dialog over the current section. */
  function onHash() {
    if (location.hash === '#pro') {
      history.replaceState(null, '', '#' + section);
      proModal.step = 'offer';
    } else section = readHash();
  }
  $effect(onHash);
  /** The Become listener or Join form, shown as a dialog. */
  let form = $state<'listen' | 'join' | null>(null);
  $effect(() => {
    if (form === 'join' && app.link) form = null;
  });
  /** Devices already sending stay connected; new ones need the code. */
  function makePrivate() {
    actions.setPrivate(newCode());
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

<svelte:window onhashchange={onHash} />

<div class="win">
  <header class="strip">
    <span class="brand">Aonin</span>
    <span class="me">{app.hosting?.name ?? app.me.name} · {app.me.os === 'macos' ? 'macOS' : 'Windows'}</span>
    <EditionBadge />
    <span class="role"><i style:background={dot}></i>{roleLine(app)}</span>
  </header>

  {#if app.hosting?.code}
    {@const code = app.hosting.code}
    <div class="vis">
      <span class="icon" aria-hidden="true"><Lock size={16} strokeWidth={1.75} /></span>
      <span class="text">
        <span class="title">{t('vis.private')}</span>
        <span class="label">{t('vis.privateHint')}</span>
      </span>
      <span class="bigcode code" aria-label={t('common.codeAria', { code: code.split('').join(' ') })}>{fmtCode(code)}</span>
      <button class="btn" onclick={() => copyCode(code)}><Copy size={14} strokeWidth={1.75} />{copied ? t('common.copied') : t('common.copyCode')}</button>
      <button class="btn" onclick={() => actions.setPrivate(null)}><Globe size={14} strokeWidth={1.75} />{t('common.makePublic')}</button>
    </div>
  {:else if app.hosting}
    <div class="vis">
      <span class="icon" aria-hidden="true"><Globe size={16} strokeWidth={1.75} /></span>
      <span class="text">
        <span class="title">{t('vis.public')}</span>
        <span class="label">{t('vis.publicHint')}</span>
      </span>
      {#if app.proUnlocked}
        <button class="btn" onclick={makePrivate}><Lock size={14} strokeWidth={1.75} />{t('common.makePrivate')}</button>
      {:else}
        <button class="btn" onclick={getPro}><Lock size={14} strokeWidth={1.75} />{t('common.makePrivate')}<span class="pro-tag">Pro</span></button>
      {/if}
    </div>
  {/if}

  <div class="body">
    <nav aria-label={t('nav.sections')}>
      {#each sections as s}
        {@const Icon = sectionIcons[s]}
        <a href="#{s}" aria-current={section === s ? 'page' : undefined}><Icon size={15} strokeWidth={1.75} aria-hidden="true" />{t(`nav.${s}`)}</a>
      {/each}
      {#if !app.proUnlocked}
        <button class="getpro" onclick={() => actions.getPro()}><Sparkles size={15} strokeWidth={1.75} aria-hidden="true" />{t('nav.getPro')}</button>
      {/if}
      <label class="lang" title={t('advanced.languageHint')}>
        <Languages size={15} strokeWidth={1.75} aria-hidden="true" />
        <span class="current" aria-hidden="true">{(LANGUAGES[i18n.lang] as { short?: string }).short ?? LANGUAGES[i18n.lang].name}</span>
        <ChevronDown size={13} strokeWidth={1.75} aria-hidden="true" />
        <select aria-label={t('advanced.language')} value={i18n.choice} onchange={(e) => setLanguage(e.currentTarget.value as LangChoice)}>
          <option value="system">{t('advanced.languageSystem', { language: LANGUAGES[systemLang()].name })}</option>
          {#each Object.entries(LANGUAGES) as [code, l]}<option value={code} lang={l.tag}>{l.name}</option>{/each}
        </select>
      </label>
      <button class="quit" onclick={actions.quit}><Power size={14} strokeWidth={1.75} />{t('common.quitAonin')}</button>
    </nav>

    <main>
      <Issues />
      {#if app.error}
        <p class="alert" role="alert">{tm(app.error)}</p>
      {/if}
      {#if app.outputLost}
        <p class="alert" role="status">{t('alert.outputLost')}</p>
      {/if}

      {#if section === 'devices'}
        {#if app.hosting}
          <div class="titlebar">
            <h2>{t('devices.audioSenders')}</h2>
            <span class="label">{t('devices.playingTo', { device: app.hosting.output })}</span>
            <button class="btn" onclick={actions.stopHosting}>{t('common.stopListening')}</button>
          </div>
          {#if app.hosting.clients.length}
            <div class="cols label"><span></span><span>{t('devices.colDevice')}</span><span>{t('devices.colLevel')}</span><span class="r">{t('devices.colLatency')}</span><span>{t('devices.colVolume')}</span><span></span><span class="btn ghost" aria-hidden="true"><ArrowLeftRight size={13} strokeWidth={1.75} />{t('common.makeListener')}{#if !app.proUnlocked}<span class="pro-tag">Pro</span>{/if}</span></div>
            {#each app.hosting.clients as row (row.id)}
              <ClientRow {row} wide lost={app.outputLost} locked={!app.proUnlocked} onmake={app.handingOver || app.hosting.code ? undefined : !app.proUnlocked ? getPro : () => (makeTarget = { id: row.id, name: row.name })} />
            {/each}
          {:else}
            <p class="empty">{app.hosting.code ? t('devices.waitingPrivate') : t('devices.waitingPublic')}</p>
          {/if}
        {/if}

        {#if app.link}
          <div class="titlebar">
            <h2>{t('devices.connection')}</h2>
            {#if !app.hosting}<button class="btn" onclick={() => (form = 'listen')}>{t('common.becomeListener')}</button>{/if}
          </div>
          <div class="panel">
            <div class="linkhead">
              <StatusMark mark={app.link.status === 'connected' ? 'live' : 'connecting'} />
              <span class="name">{app.link.host.name}</span>
              <span class="label num">{app.link.status === 'connected' ? fmtMs(app.link.latencyMs) : t('common.reconnecting')}</span>
              <button class="btn" onclick={actions.disconnect}>{t('common.disconnect')}</button>
            </div>
          </div>

          <div class="titlebar">
            <h2>{t('devices.sendTo', { name: app.link.host.name })}</h2>
            <span class="label">{t(app.me.os === 'macos' ? 'devices.sendFromMac' : 'devices.sendFromPc', { name: app.link.host.name })}</span>
          </div>
          <Switch icon={Monitor} label={t('common.systemAudio')} hint={t('devices.systemAudioHint')} checked={app.link.send.system} onchange={(on) => actions.setSend('system', on)} />
          <Switch
            icon={VolumeX}
            label={t('devices.muteSpeakers')}
            hint={t(app.me.os === 'macos' ? 'devices.muteSpeakersMac' : 'devices.muteSpeakersPc')}
            checked={app.options.muteSpeakers}
            disabled={!app.link.send.system}
            onchange={(on) => actions.setMuteSpeakers(on)}
          />
          <Switch icon={Mic} label={t('common.microphone')} hint={app.devices.mic || app.devices.defaultMic} checked={app.link.send.mic} onchange={(on) => actions.setSend('mic', on)} />

          <div class="titlebar">
            <h2>{t('devices.hearFrom', { name: app.link.host.name })}</h2>
            <span class="label">{t(app.me.os === 'macos' ? 'devices.hearOnMac' : 'devices.hearOnPc', { name: app.link.host.name })}</span>
          </div>
          <Switch icon={Monitor} label={t('common.systemAudio')} hint={t('devices.hearSystemHint', { name: app.link.host.name })} checked={app.link.hear.system} onchange={(on) => actions.setHear('system', on)} />
          <Switch icon={Mic} label={t('common.microphone')} hint={t('devices.hearMicHint', { name: app.link.host.name })} checked={app.link.hear.mic} onchange={(on) => actions.setHear('mic', on)} />
        {/if}

        {#if !app.hosting && !app.link}
          <div class="titlebar">
            <h2>{t('devices.listenersHere')}</h2>
            {#if app.hosts.length}
              <span class="pair">
                <button class="btn" onclick={() => (form = 'join')}>{t('common.joinWithCode')}</button>
                <button class="btn" onclick={() => (form = 'listen')}>{t('common.becomeListener')}</button>
              </span>
            {/if}
          </div>
          {#if app.hosts.length === 0}
            <div class="solo">
              <p>{t('devices.noListener')}</p>
              <span class="pair">
                <button class="btn primary" onclick={() => (form = 'listen')}>{t('common.becomeListener')}</button>
                <button class="btn" onclick={() => (form = 'join')}>{t('common.join')}</button>
              </span>
              <p class="label">{t('devices.joinPrivateHint')}</p>
            </div>
          {/if}
        {/if}

        {#if (app.hosting || !app.link) && others.length}
          {#if app.hosting}<div class="titlebar"><h2>{t('devices.otherListeners')}</h2></div>{/if}
          {#each others as h (h.id)}
            <div class="hostrow">
              <StatusMark mark={h.blocked ? 'error' : 'live'} />
              <div class="who">
                <span class="name">{h.name}</span>
                <span class="label" class:bad={h.blocked}>{h.blocked ? tm(h.blocked) : tn('devices.listeningSenders', h.clients)}</span>
              </div>
              {#if !h.blocked}<button class="btn" onclick={() => actions.connect(h.id)}>{t('common.connect')}</button>{/if}
            </div>
          {/each}
        {/if}
      {:else if section === 'audio'}
        <div class="titlebar"><h2>{t('audio.title')}</h2></div>
        <div class="form">
          <label><span>{t('audio.playOn')}</span>
            <select value={app.devices.output} onchange={(e) => actions.setDevice('output', e.currentTarget.value)}>
              <option value="">{t('common.systemDefault', { device: app.devices.defaultOutput || t('common.none') })}</option>
              {#each app.devices.outputs as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">{t('audio.playOnHint')}</span></label>
          <label><span>{t('audio.systemFrom')}</span>
            <select value={app.devices.systemSource} onchange={(e) => actions.setDevice('systemSource', e.currentTarget.value)}>
              <option value="">{t('common.systemDefault', { device: app.devices.defaultOutput || t('common.none') })}</option>
              {#each app.devices.outputs as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">{t('audio.systemFromHint')}</span></label>
          <label><span>{t('common.microphone')}</span>
            <select value={app.devices.mic} onchange={(e) => actions.setDevice('mic', e.currentTarget.value)}>
              <option value="">{t('common.systemDefault', { device: app.devices.defaultMic || t('common.none') })}</option>
              {#each app.devices.mics as d}<option value={d}>{d}</option>{/each}
            </select>
            <span class="label">{t('audio.micHint')}</span></label>
        </div>
      {:else if section === 'network'}
        <div class="titlebar"><h2>{t('network.health')}</h2></div>
        {#if app.stats.length}
          <table>
            <thead><tr class="label"><th></th><th>{t('devices.colDevice')}</th><th class="r">{t('devices.colLatency')}</th><th class="r">{t('network.colRepaired')}</th><th class="r">{t('network.colCovered')}</th><th class="r">{t('network.colDropouts')}</th></tr></thead>
            <tbody>
              {#each app.stats as s}
                <tr>
                  <td class="dot"><StatusMark mark={s.dropouts > 0 ? 'error' : s.concealed > 0 ? 'degraded' : 'live'} /></td>
                  <td>{s.name}</td>
                  <td class="num r">{fmtMs(s.bufferMs)} <span class="label">{t('network.target', { ms: fmtMs(s.targetMs) })}</span></td>
                  <td class="num r">{s.recovered}</td>
                  <td class="num r" class:warnc={s.concealed > 0}>{s.concealed}</td>
                  <td class="num r" class:badc={s.dropouts > 0}>{s.dropouts}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <dl class="legend">
            <dt>{t('network.colRepaired')}</dt><dd>{t('network.repairedHelp')}</dd>
            <dt>{t('network.colCovered')}</dt><dd>{t('network.coveredHelp')}</dd>
            <dt>{t('network.colDropouts')}</dt><dd>{t('network.dropoutsHelp')}</dd>
          </dl>
        {:else}
          <p class="empty">{t('network.idle')}</p>
        {/if}
        {#if app.me.os === 'macos'}
          <div class="titlebar"><h2>{t('network.localAccess')}</h2></div>
          <div class="update">
            <span class="icon" aria-hidden="true"><Network size={16} strokeWidth={1.75} /></span>
            <span class="text">
              <span>{app.issues.includes('localNetwork') ? t('network.blocked') : t('network.reachable')}</span>
              {#if localNet.ask === 'asking'}
                <span class="label">{t('network.asking')}</span>
              {:else if localNet.ask === 'done' && app.issues.includes('localNetwork')}
                <span class="label bad">{t('network.didntAsk')}</span>
              {:else}
                <span class="label">{t('network.askHelp')}</span>
              {/if}
            </span>
            <button class="btn" onclick={actions.askLocalNetwork} disabled={localNet.ask === 'asking'}>{localNet.ask === 'asking' ? t('network.askingShort') : t('network.askAgain')}</button>
            <button class="btn" class:primary={localNet.ask === 'done' && app.issues.includes('localNetwork')} onclick={() => actions.fixIssue('localNetwork')}>{t('common.openSettings')}</button>
          </div>
          <div class="titlebar"><h2>{t('network.wifi')}</h2></div>
          <Switch icon={Wifi} label={t('network.steadyWifi')} hint={t('network.steadyWifiHint')} checked={app.options.quietWifi} onchange={(on) => actions.setQuietWifi(on)} />
        {/if}
      {:else if section === 'about'}
        <div class="about">
          <img src="/favicon.png" alt="" width="64" height="64" />
          <div>
            <h2>Aonin</h2>
            <p class="label">{t('about.version', { version: app.version })}</p>
          </div>
        </div>
        <p class="overview">{t('about.overview')}</p>

        <div class="titlebar"><h2>{t('about.updates')}</h2></div>
        <div class="update">
          <span class="icon" aria-hidden="true"><Download size={16} strokeWidth={1.75} /></span>
          <span class="text">
            {#if app.update}
              <span>{t('about.updateReady', { version: app.update })}</span>
              <span class="label">{t('about.updateReadyHint')}</span>
            {:else if check === 'checking'}
              <span>{t('about.checking')}</span>
              <span class="label">{t('about.checkingHint')}</span>
            {:else if check === 'latest'}
              <span>{t('about.latest', { version: app.version })}</span>
              <span class="label">{t('about.latestHint')}</span>
            {:else if check === 'failed'}
              <span>{t('about.checkFailed')}</span>
              <span class="label bad">{t('about.checkFailedHint')}</span>
            {:else}
              <span>{t('about.current', { version: app.version })}</span>
              <span class="label">{t('about.currentHint')}</span>
            {/if}
          </span>
          {#if app.update}
            <button class="btn primary" onclick={actions.installUpdate}>{t('about.restartToUpdate')}</button>
          {:else}
            <button class="btn" onclick={checkNow} disabled={check === 'checking'}>{t('about.checkForUpdates')}</button>
          {/if}
        </div>

        <div class="titlebar"><h2>{t('pro.title')}</h2></div>
        <div class="procard">
          {#if app.proUnlocked && app.license}
            <div class="prohead">
              <span class="pro-tag xl">Pro</span>
              <span class="text">
                <span class="strong">{t('pro.unlocked')}</span>
                <span class="label">{t('pro.licenseLine', { id: app.license.id, date: fmtDate(app.license.updatesUntil) })}</span>
              </span>
              <button class="btn quiet" onclick={actions.clearLicense}>{t('pro.removeKey')}</button>
            </div>
          {:else if app.proReady}
            <div class="prohead">
              <span class="pro-tag xl">Pro</span>
              <span class="text">
                <span class="strong">{t('pro.ready')}</span>
                <span class="label">{t('pro.readyHint')}</span>
              </span>
              <button class="btn primary" onclick={actions.installUpdate}>{t('pro.restartToFinish')}</button>
            </div>
          {:else if app.license && app.edition === 'pro'}
            <div class="prohead">
              <span class="pro-tag xl">Pro</span>
              <span class="text">
                <span class="strong">{t('pro.updatesEnded', { date: fmtDate(app.license.updatesUntil) })}</span>
                <span class="label">{t('pro.updatesEndedHint')}</span>
              </span>
              <button class="btn primary" onclick={() => actions.openUrl(PRO_LINKS.buy)}>{t('pro.renew')}</button>
            </div>
          {:else}
            <div class="prohead">
              <span class="pro-tag xl">Pro</span>
              <span class="text">
                <span class="strong">{t('pro.unlockTitle')}</span>
                <span class="label">{t('pro.offer')}</span>
              </span>
            </div>
            <ProFeatures />
            <ListenerNote />
            <div class="proactions">
              {#if app.license}
                <button class="btn primary" onclick={() => actions.getPro('key')}>{t('pro.download')}</button>
                <span class="label">{t('pro.licenseId', { id: app.license.id })}</span>
              {:else}
                <button class="btn primary" onclick={() => actions.openUrl(PRO_LINKS.buy)}>{t('pro.purchase')}</button>
                <button class="btn" onclick={() => actions.getPro('key')}>{t('pro.enterKey')}</button>
              {/if}
            </div>
          {/if}
        </div>

        <div class="titlebar"><h2>{t('about.license')}</h2></div>
        <p class="legal">{t('about.legal')}</p>
        <p class="links">
          <button class="link" onclick={() => actions.openUrl(REPO)}>{t('about.source')}</button>
          <button class="link" onclick={() => actions.openUrl(REPO + '/blob/main/LICENSE')}>{t('about.readLicense')}</button>
        </p>
      {:else}
        <div class="titlebar"><h2>{t('advanced.quality')}</h2><span class="label">{t('advanced.qualityHint')}</span></div>
        <div class="radios" role="radiogroup" aria-label={t('advanced.quality')}>
          {#each Object.entries(QUALITY) as [key, q]}
            <label class="radio">
              <input type="radio" name="quality" value={key} checked={app.quality === key} onchange={() => actions.setQuality(key as Quality)} />
              <span class="dotbox" aria-hidden="true"></span>
              <span class="qname">{t(`quality.${key}` as 'quality.cd')}</span>
              <span class="label">{t(`quality.${key}Detail` as 'quality.cdDetail')}</span>
              <span class="label num r">{t('quality.rate', { rate: q.rate })}</span>
            </label>
          {/each}
        </div>
        <div class="titlebar"><h2>{t('advanced.startup')}</h2></div>
        <Switch icon={RefreshCw} label={t('advanced.reconnect')} hint={t('advanced.reconnectHint')} checked={app.options.reconnect} onchange={(on) => actions.setReconnect(on)} />
        <Switch icon={LogIn} label={t('advanced.openAtLogin')} hint={t('advanced.openAtLoginHint')} checked={app.options.startAtLogin} onchange={(on) => actions.setStartAtLogin(on)} />
      {/if}
    </main>
  </div>
</div>

{#if form}
  <Modal label={form === 'listen' ? t('listen.title') : t('join.title')} onclose={() => { if (form === 'join') actions.cancelJoin(); form = null; }}>
    {#if form === 'listen'}
      <ListenForm oncancel={() => (form = null)} ondone={() => (form = null)} />
    {:else}
      <JoinForm oncancel={() => (form = null)} />
    {/if}
  </Modal>
{/if}
{#if proModal.step}
  <ProModal />
{/if}
{#if makeTarget}
  {@const mt = makeTarget}
  <Confirm
    title={t('make.question', { name: mt.name })}
    body={t(app.me.os === 'macos' ? 'make.bodyMac' : 'make.bodyPc', { name: mt.name })}
    action={t('common.makeListener')}
    onconfirm={() => actions.handOver(mt.id)}
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
  .getpro { display: flex; align-items: center; gap: 10px; margin-top: 6px; padding: 8px 10px; border: 0; border-radius: 6px; background: transparent; color: var(--ink); font: inherit; text-align: left; cursor: pointer; }
  .getpro :global(svg) { color: var(--orange); flex: none; }
  .getpro:hover { background: color-mix(in srgb, var(--recess) 50%, transparent); }
  .procard { display: flex; flex-direction: column; gap: 14px; max-width: 560px; padding: 16px 18px; border: 1px solid var(--line); border-radius: 12px; background: var(--raised); box-shadow: 0 1px 2px rgb(0 0 0 / 0.06); }
  .prohead { display: flex; align-items: center; gap: 12px; }
  .prohead .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .strong { font-weight: 700; }
  .proactions { display: flex; align-items: center; gap: 8px; }
  /* Language: shows the current one; the real select sits on top, see-through. */
  .lang { position: relative; margin-top: auto; display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 6px; color: var(--ink-2); cursor: pointer; }
  .lang .current { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lang:hover { background: var(--recess); color: var(--ink); }
  .lang:focus-within { outline: 2px solid var(--focus); outline-offset: -2px; }
  .lang select { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; font: inherit; }
  .quit {
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
  .cols { display: grid; grid-template-columns: 8px minmax(96px, 1fr) 105px 64px 100px 28px minmax(124px, auto); gap: 12px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
  .r { text-align: right; }
  /* Same width as the Make listener buttons below, so the columns line up. */
  .ghost { visibility: hidden; height: 0; padding: 0 13px; border: 0; display: inline-flex; gap: 6px; white-space: nowrap; overflow: hidden; }
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
