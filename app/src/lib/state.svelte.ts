// App state shared by the popover and the main window. In the Tauri app the
// backend owns it and pushes snapshots; in a plain browser (design review)
// a demo scenario is loaded from `?demo=<name>`.

import { t, tn } from './i18n/index.svelte';

export type Os = 'windows' | 'macos';
export type Quality = 'highest' | 'cd' | 'balanced' | 'saver';

/** One fixed mark per state, always drawn in the same cell of a row. */
export type Mark = 'live' | 'muted' | 'connecting' | 'degraded' | 'error';

export interface ClientRow {
  id: string;
  name: string;
  os: Os;
  sends: { system: boolean; mic: boolean };
  mark: Mark;
  latencyMs: number;
  /** Packets repaired by redundancy/retransmit in the last few seconds. */
  recovered: number;
  /** 0..1 */
  level: number;
  volume: number;
  muted: boolean;
}

export interface HostInfo {
  id: string;
  name: string;
  os: Os;
  clients: number;
  /** Set when joining would loop audio back to this device. */
  blocked?: string;
}

export interface Link {
  host: HostInfo;
  status: 'connected' | 'reconnecting';
  latencyMs: number;
  send: { system: boolean; mic: boolean };
  hear: { system: boolean; mic: boolean };
  /** What comes back from the host: meter level 0..1 and buffer depth. */
  hearLevel?: number;
  /** Level of what this device sends, 0..1. */
  sendLevel?: number;
  hearLatencyMs?: number;
}

export interface Stats {
  name: string;
  bufferMs: number;
  targetMs: number;
  recovered: number;
  concealed: number;
  dropouts: number;
}

export type Issue = 'localNetwork' | 'audioCapture' | 'microphone' | 'firewall';

export interface AppState {
  me: { name: string; os: Os };
  /** What the OS is stopping Aonin from doing; each shows with its fix. */
  issues: Issue[];
  /** `name`: what audio senders see. `code`: set when private. */
  hosting: null | { name: string; code: string | null; output: string; clients: ClientRow[] };
  link: null | Link;
  hosts: HostInfo[];
  quality: Quality;
  outputLost: boolean;
  stats: Stats[];
  /** Chosen device names; '' follows the system default. */
  devices: { outputs: string[]; mics: string[]; output: string; mic: string; systemSource: string; defaultOutput: string; defaultMic: string };
  options: { reconnect: boolean; startAtLogin: boolean; quietWifi: boolean; muteSpeakers: boolean };
  /** Hosting or discovery failed to start. */
  error?: string | null;
  /** This build's version, and a newer one downloaded and ready to install. */
  version: string;
  update: string | null;
  /** Name of the device this listener is handing its role to. */
  handingOver: string | null;
  /** Looking for the private listener with the code the user typed. */
  joining: boolean;
  joinError: string | null;
  /** Which build this is, and whether its Pro features are unlocked. */
  edition: 'free' | 'pro';
  proUnlocked: boolean;
  /** The Aonin Pro license entered here (already checked). */
  license: null | { id: string; updatesUntil: string };
  /** Free build: Aonin Pro is downloaded and waits for a restart. */
  proReady: boolean;
}

/** No 0/O or 1/I: a code is read off one screen and typed on another.
 *  Matches `CODE_ALPHABET` in crates/aon/src/private.rs. */
const CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';

export function newCode(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(6));
  return Array.from(bytes, (b) => CODE_ALPHABET[b % CODE_ALPHABET.length]).join('');
}

/** The code as typed ("k7q 4mz") in canonical form, or null. */
export function normalizeCode(typed: string): string | null {
  const c = typed.replace(/[\s-]/g, '').toUpperCase();
  return c.length === 6 && [...c].every((ch) => CODE_ALPHABET.includes(ch)) ? c : null;
}

/** 'K7Q4MZ' reads as "K7Q 4MZ". */
export function fmtCode(code: string): string {
  return `${code.slice(0, 3)} ${code.slice(3)}`;
}

export const QUALITY: Record<Quality, { label: string; detail: string; rate: string }> = {
  highest: { label: 'Highest', detail: '24-bit · 48 kHz, uncompressed', rate: '2.3 Mbps per stream' },
  cd: { label: 'CD', detail: '16-bit · 48 kHz, CD quality', rate: '1.5 Mbps per stream' },
  balanced: { label: 'Balanced', detail: 'Opus, for busy Wi-Fi · adds about 12 ms', rate: '256 kbps per stream' },
  saver: { label: 'Saver', detail: 'Opus, for weak Wi-Fi · adds about 12 ms', rate: '96 kbps per stream' },
};

// ---- demo scenarios (design review only; never shown in the Tauri app) ----

const base: AppState = {
  me: { name: 'PC-Studio', os: 'windows' },
  issues: [],
  hosting: null,
  link: null,
  hosts: [],
  handingOver: null,
  joining: false,
  joinError: null,
  quality: 'highest',
  outputLost: false,
  stats: [],
  devices: {
    outputs: ['Speakers (USB DAC)', 'Headphones (USB Headset)', 'Display Audio'],
    mics: ['USB Microphone', 'Headset Microphone', 'Built-in Microphone'],
    output: '',
    mic: '',
    systemSource: '',
    defaultOutput: 'Speakers (USB DAC)',
    defaultMic: 'USB Microphone',
  },
  options: { reconnect: true, startAtLogin: true, quietWifi: false, muteSpeakers: true },
  version: '0.1.1',
  update: null,
  edition: 'free',
  proUnlocked: false,
  license: null,
  proReady: false,
};

const demoLicense = { id: 'A1B2C3D4', updatesUntil: '2027-11-04' };

const clients: ClientRow[] = [
  { id: 'a', name: "Studio MacBook Pro", os: 'macos', sends: { system: true, mic: true }, mark: 'live', latencyMs: 21.8, recovered: 0, level: 0.62, volume: 0.72, muted: false },
  { id: 'b', name: 'Office MacBook', os: 'macos', sends: { system: true, mic: false }, mark: 'live', latencyMs: 24.1, recovered: 3, level: 0.38, volume: 0.55, muted: false },
  { id: 'c', name: 'Work Laptop', os: 'windows', sends: { system: true, mic: false }, mark: 'muted', latencyMs: 18.0, recovered: 0, level: 0, volume: 0.8, muted: true },
];

const pcStudio: HostInfo = { id: 'h1', name: 'PC-Studio', os: 'windows', clients: 2 };

export const DEMOS: Record<string, Partial<AppState>> = {
  host: {
    hosting: { name: 'PC-Studio', code: null, output: 'USB DAC', clients },
    hosts: [{ id: 'h2', name: 'Studio iMac', os: 'macos', clients: 1, blocked: 'Joining would play your own audio back to you.' }],
    stats: [
      { name: "Studio MacBook Pro", bufferMs: 21.8, targetMs: 20, recovered: 4, concealed: 0, dropouts: 0 },
      { name: 'Office MacBook', bufferMs: 24.1, targetMs: 23, recovered: 11, concealed: 0, dropouts: 1 },
      { name: 'Work Laptop', bufferMs: 18.0, targetMs: 18, recovered: 0, concealed: 0, dropouts: 0 },
    ],
  },
  private: { hosting: { name: 'Studio room', code: 'K7Q4MZ', output: 'USB DAC', clients: clients.slice(0, 2) } },
  waiting: { hosting: { name: 'PC-Studio', code: null, output: 'USB DAC', clients: [] } },
  // Aonin Pro, unlocked; and the free build waiting to restart into Pro.
  pro: { edition: 'pro', proUnlocked: true, license: demoLicense, hosting: { name: 'PC-Studio', code: null, output: 'USB DAC', clients } },
  proready: { license: demoLicense, proReady: true },
  client: {
    update: '0.1.2',
    me: { name: 'Office MacBook', os: 'macos' },
    link: { host: pcStudio, status: 'connected', latencyMs: 24.1, send: { system: true, mic: false }, hear: { system: false, mic: true }, sendLevel: 0.55 },
    hosts: [pcStudio, { id: 'h3', name: 'Living Room PC', os: 'windows', clients: 0 }],
    stats: [{ name: 'PC-Studio', bufferMs: 24.1, targetMs: 23, recovered: 11, concealed: 0, dropouts: 1 }],
  },
  both: {
    me: { name: 'Office MacBook', os: 'macos' },
    hosting: { name: 'PC-Studio', code: null, output: 'MacBook Pro Speakers', clients: [clients[2]] },
    link: { host: pcStudio, status: 'connected', latencyMs: 24.1, send: { system: true, mic: false }, hear: { system: false, mic: false } },
    hosts: [pcStudio],
  },
  reconnecting: {
    me: { name: 'Office MacBook', os: 'macos' },
    link: { host: pcStudio, status: 'reconnecting', latencyMs: 0, send: { system: true, mic: false }, hear: { system: false, mic: false } },
    hosts: [],
  },
  found: {
    me: { name: 'Work Laptop', os: 'windows' },
    hosts: [pcStudio, { id: 'h2', name: 'Studio iMac', os: 'macos', clients: 1, blocked: 'Joining would play your own audio back to you.' }],
  },
  empty: { me: { name: 'Work Laptop', os: 'windows' } },
  issues: { me: { name: 'Office MacBook', os: 'macos' }, issues: ['localNetwork', 'audioCapture'] },
  firewall: { hosting: { name: 'PC-Studio', code: null, output: 'USB DAC', clients: [] }, issues: ['firewall'] },
  degraded: {
    hosting: {
      name: 'PC-Studio',
      code: null,
      output: 'USB DAC',
      clients: [clients[0], { ...clients[1], mark: 'degraded', latencyMs: 68.4, recovered: 41 }],
    },
  },
  lost: { hosting: { name: 'PC-Studio', code: null, output: 'USB DAC', clients: clients.slice(0, 2) }, outputLost: true },
  crowded: {
    hosting: {
      name: 'PC-Studio',
      code: null,
      output: 'USB DAC',
      clients: [
        ...clients,
        { ...clients[0], id: 'd', name: 'Design Team MacBook Air (2nd floor)', level: 0.2 },
        { ...clients[1], id: 'e', name: 'Living Room PC', os: 'windows', level: 0.81 },
        { ...clients[1], id: 'f', name: 'Kitchen iPad Mac', mark: 'connecting', level: 0 },
      ],
    },
  },
};

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function load(): AppState {
  // In the app the backend pushes the real state; start empty until it does.
  if (inTauri) return structuredClone({ ...base, me: { name: '', os: navigator.userAgent.includes('Mac') ? 'macos' : 'windows' }, stats: [] });
  const name = typeof location === 'undefined' ? 'host' : new URLSearchParams(location.search).get('demo') ?? 'host';
  return structuredClone({ ...base, ...(DEMOS[name] ?? DEMOS.host) });
}

export const app = $state<AppState>(load());

/** Follow the engine: each `state` event replaces the whole snapshot. */
/** macOS Ask again: asking for a few seconds, then done. macOS shows its
 *  Local Network prompt only once per app, so "done" while still blocked
 *  means the answer is in Settings. */
export const localNet = $state<{ ask: 'idle' | 'asking' | 'done' }>({ ask: 'idle' });

export async function followBackend() {
  if (!inTauri) return;
  const { listen } = await import('@tauri-apps/api/event');
  await listen<AppState>('state', (e) => Object.assign(app, e.payload));
}

/** A device reads as "connected somewhere" in the strip. `short`: the
 *  narrow menu strip, where the device count is shown below anyway. */
export function roleLine(s: AppState, short = false): string {
  if (s.issues.length) return t('role.needsPermission');
  if (s.handingOver) return short ? t('role.handingOverShort') : t('role.handingOver', { name: s.handingOver });
  if (s.hosting && s.outputLost) return t('role.outputLost');
  if (s.hosting && s.link) return short ? t('role.listening') : t('role.listeningAlsoSending');
  if (s.hosting) {
    if (short) return t('role.listening');
    const n = s.hosting.clients.length;
    return n === 0 ? t('role.listeningNoSenders') : tn('role.listeningSenders', n);
  }
  if (s.link) return s.link.status === 'connected' ? t('role.connected') : t('common.reconnecting');
  return t('role.notConnected');
}

export function fmtMs(ms: number): string {
  return `${ms.toFixed(1)} ms`;
}

/** The Unlock Pro dialog in the main window: the offer, or the key field. */
export const proModal = $state<{ step: null | 'offer' | 'key' }>({ step: null });
/** Where to buy Aonin Pro (Aonin's page on lfg.ink), and where to ask for a key. */
export const PRO_LINKS = { buy: 'https://lfg.ink/aonin', request: 'https://discord.gg/2cQBYaNHjC' };
