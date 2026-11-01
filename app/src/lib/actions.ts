// User intents. In the Tauri app each one becomes a backend command; in the
// browser design review they mutate the demo state so the UI stays clickable.

import { app, inTauri } from './state.svelte';

export type Section = 'devices' | 'audio' | 'network' | 'advanced';


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
    if (!(await call('become_host'))) app.hosting = { output: app.devices.output, clients: [] };
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
  async grantPermission() {
    if (!(await call('request_permission'))) app.gate = null;
  },
  async allowFirewall() {
    if (!(await call('allow_firewall'))) app.gate = null;
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
  async installUpdate() {
    await call('install_update');
  },
  async setQuality(quality: 'lossless' | 'cd' | 'balanced' | 'saver') {
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
