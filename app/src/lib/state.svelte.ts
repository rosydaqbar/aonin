// App state shared by the popover and the main window. In the Tauri app the
// backend owns it and pushes snapshots; in a plain browser (design review)
// a demo scenario is loaded from `?demo=<name>`.

export type Os = 'windows' | 'macos';
export type Quality = 'lossless' | 'cd' | 'balanced' | 'saver';

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
}

export interface Stats {
  name: string;
  bufferMs: number;
  targetMs: number;
  recovered: number;
  concealed: number;
  dropouts: number;
}

export interface AppState {
  me: { name: string; os: Os };
  gate: null | 'permission' | 'firewall';
  searching: boolean;
  hosting: null | { output: string; clients: ClientRow[] };
  link: null | Link;
  hosts: HostInfo[];
  quality: Quality;
  outputLost: boolean;
  stats: Stats[];
  devices: { outputs: string[]; mics: string[]; output: string; mic: string; systemSource: string };
  options: { reconnect: boolean; startAtLogin: boolean; quietWifi: boolean };
  /** Hosting or discovery failed to start. */
  error?: string | null;
}

export const QUALITY: Record<Quality, { label: string; detail: string; rate: string }> = {
  lossless: { label: 'Lossless', detail: '24-bit · 48 kHz, identical to the source', rate: '2.3 Mbps per stream' },
  cd: { label: 'CD', detail: '16-bit · 48 kHz, CD quality', rate: '1.5 Mbps per stream' },
  balanced: { label: 'Balanced', detail: 'Opus, for busy Wi-Fi', rate: '256 kbps per stream' },
  saver: { label: 'Saver', detail: 'Opus, for weak Wi-Fi', rate: '96 kbps per stream' },
};

// ---- demo scenarios (design review only; never shown in the Tauri app) ----

const base: AppState = {
  me: { name: 'PC-Studio', os: 'windows' },
  gate: null,
  searching: false,
  hosting: null,
  link: null,
  hosts: [],
  quality: 'lossless',
  outputLost: false,
  stats: [],
  devices: {
    outputs: ['Speakers (USB DAC)', 'Headphones (USB Headset)', 'Display Audio'],
    mics: ['USB Microphone', 'Headset Microphone', 'Built-in Microphone'],
    output: 'Speakers (USB DAC)',
    mic: 'USB Microphone',
    systemSource: 'Speakers (USB DAC)',
  },
  options: { reconnect: true, startAtLogin: true, quietWifi: false },
};

const clients: ClientRow[] = [
  { id: 'a', name: "Studio MacBook Pro", os: 'macos', sends: { system: true, mic: true }, mark: 'live', latencyMs: 21.8, recovered: 0, level: 0.62, volume: 0.72, muted: false },
  { id: 'b', name: 'Office MacBook', os: 'macos', sends: { system: true, mic: false }, mark: 'live', latencyMs: 24.1, recovered: 3, level: 0.38, volume: 0.55, muted: false },
  { id: 'c', name: 'Work Laptop', os: 'windows', sends: { system: true, mic: false }, mark: 'muted', latencyMs: 18.0, recovered: 0, level: 0, volume: 0.8, muted: true },
];

const pcStudio: HostInfo = { id: 'h1', name: 'PC-Studio', os: 'windows', clients: 2 };

export const DEMOS: Record<string, Partial<AppState>> = {
  host: {
    hosting: { output: 'USB DAC', clients },
    hosts: [{ id: 'h2', name: 'Studio iMac', os: 'macos', clients: 1, blocked: 'Its audio already comes from this PC.' }],
    stats: [
      { name: "Studio MacBook Pro", bufferMs: 21.8, targetMs: 20, recovered: 4, concealed: 0, dropouts: 0 },
      { name: 'Office MacBook', bufferMs: 24.1, targetMs: 23, recovered: 11, concealed: 0, dropouts: 1 },
      { name: 'Work Laptop', bufferMs: 18.0, targetMs: 18, recovered: 0, concealed: 0, dropouts: 0 },
    ],
  },
  waiting: { hosting: { output: 'USB DAC', clients: [] } },
  client: {
    me: { name: 'Office MacBook', os: 'macos' },
    link: { host: pcStudio, status: 'connected', latencyMs: 24.1, send: { system: true, mic: false }, hear: { system: false, mic: true } },
    hosts: [pcStudio, { id: 'h3', name: 'Living Room PC', os: 'windows', clients: 0 }],
    stats: [{ name: 'PC-Studio', bufferMs: 24.1, targetMs: 23, recovered: 11, concealed: 0, dropouts: 1 }],
  },
  both: {
    me: { name: 'Office MacBook', os: 'macos' },
    hosting: { output: 'MacBook Pro Speakers', clients: [clients[2]] },
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
    hosts: [pcStudio, { id: 'h2', name: 'Studio iMac', os: 'macos', clients: 1, blocked: 'Its audio already comes from this laptop.' }],
  },
  empty: { me: { name: 'Work Laptop', os: 'windows' } },
  searching: { me: { name: 'Work Laptop', os: 'windows' }, searching: true },
  permission: { me: { name: 'Office MacBook', os: 'macos' }, gate: 'permission' },
  firewall: { gate: 'firewall' },
  degraded: {
    hosting: {
      output: 'USB DAC',
      clients: [clients[0], { ...clients[1], mark: 'degraded', latencyMs: 68.4, recovered: 41 }],
    },
  },
  lost: { hosting: { output: 'USB DAC', clients: clients.slice(0, 2) }, outputLost: true },
  crowded: {
    hosting: {
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
  if (inTauri) return structuredClone({ ...base, me: { name: '', os: navigator.userAgent.includes('Mac') ? 'macos' : 'windows' }, searching: true, stats: [] });
  const name = typeof location === 'undefined' ? 'host' : new URLSearchParams(location.search).get('demo') ?? 'host';
  return structuredClone({ ...base, ...(DEMOS[name] ?? DEMOS.host) });
}

export const app = $state<AppState>(load());

/** Follow the engine: each `state` event replaces the whole snapshot. */
export async function followBackend() {
  if (!inTauri) return;
  const { listen } = await import('@tauri-apps/api/event');
  await listen<AppState>('state', (e) => Object.assign(app, e.payload));
}

/** A device reads as "connected somewhere" in the strip. */
export function roleLine(s: AppState): string {
  if (s.gate) return 'setup needed';
  if (s.hosting && s.link) return `hosting · also sending to ${s.link.host.name}`;
  if (s.hosting) {
    const n = s.hosting.clients.length;
    return n === 0 ? 'hosting · no clients yet' : `hosting · ${n} ${n === 1 ? 'client' : 'clients'}`;
  }
  if (s.link) return s.link.status === 'connected' ? `sending to ${s.link.host.name}` : `reconnecting to ${s.link.host.name}`;
  if (s.searching) return 'looking for hosts';
  return 'not connected';
}

export function fmtMs(ms: number): string {
  return `${ms.toFixed(1)} ms`;
}
