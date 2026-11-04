// User intents. In the Tauri app each one becomes a backend command; in the
// browser design review they mutate the demo state so the UI stays clickable.

import { app, inTauri, localNet, normalizeCode, type Issue } from './state.svelte';

export type Section = 'devices' | 'audio' | 'network' | 'advanced' | 'about';


async function call(cmd: string, args?: Record<string, unknown>): Promise<boolean> {
  if (!inTauri) return false;
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    await invoke(cmd, args);
  } catch (e) {
    app.error = String(e);
  }
  return true;
}

export const actions = {
  async becomeHost() {
    if (!(await call('become_host'))) app.hosting = { name: app.me.name, code: null, output: app.devices.output, clients: [] };
  },
  /** Start listening as `name`, privately when `code` is set. */
  async startListening(name: string, code: string | null) {
    if (await call('start_listening', { name, code })) return;
    app.hosting = { name: name.trim() || app.me.name, code, output: app.devices.output || app.devices.defaultOutput, clients: [] };
  },
  /** Make the running listener private with `code`, or public with null. */
  async setPrivate(code: string | null) {
    if (!(await call('set_private', { code })) && app.hosting) app.hosting.code = code;
  },
  /** Join the private listener showing `code`; the answer arrives in the state. */
  async joinCode(code: string) {
    app.joinError = null;
    if (await call('join_code', { code })) return;
    // Design review: "K7Q4MZ" is the one private listener in the demo.
    app.joining = true;
    setTimeout(() => {
      app.joining = false;
      if (normalizeCode(code) === 'K7Q4MZ') {
        app.link = { host: { id: 'p1', name: 'Studio room', os: 'windows', clients: 1 }, status: 'connected', latencyMs: 21.4, send: { system: true, mic: false }, hear: { system: false, mic: false } };
      } else {
        app.joinError = "That code didn't work. Check it and try again.";
      }
    }, 900);
  },
  async cancelJoin() {
    app.joinError = null;
    if (!(await call('cancel_join'))) app.joining = false;
  },
  async handOver(clientId: string) {
    if (await call('hand_over', { clientId })) return;
    app.handingOver = app.hosting?.clients.find((c) => c.id === clientId)?.name ?? null;
  },
  async stopHosting() {
    if (!(await call('stop_hosting'))) app.hosting = null;
  },
  async connect(hostId: string) {
    if (await call('connect', { hostId })) return;
    const host = app.hosts.find((h) => h.id === hostId);
    if (host) app.link = { host, status: 'connected', latencyMs: 21.4, send: { system: true, mic: false }, hear: { system: false, mic: false } };
  },
  async disconnect() {
    if (!(await call('disconnect'))) app.link = null;
  },
  /** macOS: make the Local Network prompt appear again. */
  async askLocalNetwork() {
    localNet.ask = 'asking';
    await call('ask_local_network');
    // The probe runs for about five seconds; the prompt, if any, shows by then.
    setTimeout(() => (localNet.ask = 'done'), 6000);
  },
  async fixIssue(issue: Issue) {
    if (!(await call('fix_issue', { issue }))) app.issues = app.issues.filter((i) => i !== issue);
  },

  async openWindow(section: Section) {
    if (!(await call('open_window', { section }))) location.href = `/?demo=${new URLSearchParams(location.search).get('demo') ?? 'host'}#${section}`;
  },
  async setVolume(client: string, volume: number) {
    if (!(await call('set_volume', { client, volume }))) {
      const row = app.hosting?.clients.find((c) => c.id === client);
      if (row) row.volume = volume;
    }
  },
  async setMuted(client: string, muted: boolean) {
    if (!(await call('set_muted', { client, muted }))) {
      const row = app.hosting?.clients.find((c) => c.id === client);
      if (row) row.muted = muted;
    }
  },
  async setSend(source: 'system' | 'mic', on: boolean) {
    if (!(await call('set_send', { source, on })) && app.link) app.link.send[source] = on;
  },
  async setHear(source: 'system' | 'mic', on: boolean) {
    if (!(await call('set_hear', { source, on })) && app.link) app.link.hear[source] = on;
  },
  async setStartAtLogin(on: boolean) {
    if (!(await call('set_start_at_login', { on }))) app.options.startAtLogin = on;
  },
  async setQuietWifi(on: boolean) {
    if (!(await call('set_quiet_wifi', { on }))) app.options.quietWifi = on;
  },
  async setMuteSpeakers(on: boolean) {
    if (!(await call('set_mute_speakers', { on }))) app.options.muteSpeakers = on;
  },
  /** The version ready to install, null when up to date; throws if the check failed. */
  async checkUpdate(): Promise<string | null> {
    if (!inTauri) return app.update;
    const { invoke } = await import('@tauri-apps/api/core');
    const version = await invoke<string | null>('check_update');
    app.update = version;
    return version;
  },
  async openUrl(url: string) {
    if (!inTauri) return void window.open(url, '_blank');
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  },
  /** About → Aonin Pro: check and keep the key. Throws with the reason. */
  async setLicense(key: string) {
    if (!inTauri) {
      if (!/^AONIN-[w-]{90,}$/.test(key.replace(/s/g, ''))) throw new Error("That license key isn't valid. Check it and try again.");
      app.license = { id: 'A1B2C3D4', updatesUntil: '2027-11-04' };
      if (app.edition === 'pro') app.proUnlocked = true;
      return;
    }
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('set_license', { key });
  },
  async clearLicense() {
    if (!(await call('clear_license'))) {
      app.license = null;
      app.proUnlocked = false;
    }
  },
  /** Free build: download Aonin Pro; throws with the reason. */
  async unlockPro() {
    if (!inTauri) {
      await new Promise((r) => setTimeout(r, 1200));
      app.proReady = true;
      return;
    }
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('unlock_pro');
  },
  async installUpdate() {
    await call('install_update');
  },
  async setQuality(quality: 'highest' | 'cd' | 'balanced' | 'saver') {
    if (!(await call('set_quality', { quality }))) app.quality = quality;
  },
  async setDevice(role: 'output' | 'systemSource' | 'mic', name: string) {
    if (!(await call('set_device', { role, name }))) app.devices[role] = name;
  },
  async setReconnect(on: boolean) {
    if (!(await call('set_reconnect', { on }))) app.options.reconnect = on;
  },
  async quit() {
    await call('quit');
  },
};
