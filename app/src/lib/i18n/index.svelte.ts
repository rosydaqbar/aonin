// Languages: pick, remember, translate. English is the source (en.ts); a
// key missing from another language falls back to English.

import { en, type Dict, type Key } from './en';
import { es } from './es';
import { fr } from './fr';
import { id } from './id';
import { RUST } from './rust';
import { zh } from './zh';

export const LANGUAGES = {
  en: { name: 'English', dict: en as Dict, tag: 'en' },
  id: { name: 'Bahasa Indonesia', short: 'Indonesia', dict: id, tag: 'id' },
  fr: { name: 'Français', dict: fr, tag: 'fr' },
  es: { name: 'Español', dict: es, tag: 'es' },
  zh: { name: '简体中文', dict: zh, tag: 'zh-CN' },
} satisfies Record<string, { name: string; short?: string; dict: Partial<Dict>; tag: string }>;
export type Lang = keyof typeof LANGUAGES;
/** 'system' follows the OS language. */
export type LangChoice = Lang | 'system';

const STORE = 'aonin.language';

/** The OS language, if Aonin has it; English otherwise. */
export function systemLang(): Lang {
  const prefs = typeof navigator === 'undefined' ? [] : [...(navigator.languages ?? []), navigator.language];
  for (const p of prefs) {
    const base = p?.toLowerCase().split('-')[0];
    if (base === 'in') return 'id';
    if (base && base in LANGUAGES) return base as Lang;
  }
  return 'en';
}

function readChoice(): LangChoice {
  try {
    const v = localStorage.getItem(STORE);
    if (v === 'system' || (v && v in LANGUAGES)) return v as LangChoice;
  } catch {}
  return 'system';
}

export const i18n = $state<{ choice: LangChoice; lang: Lang }>({ choice: 'system', lang: 'en' });

function apply(choice: LangChoice) {
  i18n.choice = choice;
  i18n.lang = choice === 'system' ? systemLang() : choice;
  if (typeof document !== 'undefined') document.documentElement.lang = LANGUAGES[i18n.lang].tag;
}

/** Call once per window. The other window follows through `storage`. */
export function initLanguage() {
  const demo = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('lang');
  apply(demo && demo in LANGUAGES ? (demo as Lang) : readChoice());
  if (typeof window !== 'undefined') window.addEventListener('storage', (e) => e.key === STORE && apply(readChoice()));
}

export function setLanguage(choice: LangChoice) {
  try {
    localStorage.setItem(STORE, choice);
  } catch {}
  apply(choice);
}

type Vars = Record<string, string | number>;

function fill(text: string, vars?: Vars): string {
  return vars ? text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : text;
}

/** The text for `key` in the current language. */
export function t(key: Key, vars?: Vars): string {
  const dict = LANGUAGES[i18n.lang].dict as Partial<Dict>;
  return fill(dict[key] ?? en[key], vars);
}

/** Pick `<base>_one` or `<base>_other` by `n`. */
export function tn(base: string, n: number, vars?: Vars): string {
  const form = new Intl.PluralRules(LANGUAGES[i18n.lang].tag).select(n) === 'one' ? 'one' : 'other';
  return t(`${base}_${form}` as Key, { n, ...vars });
}

/** A date such as "4 Nov 2027" in the current language. */
export function fmtDate(day: string): string {
  return new Date(day + 'T00:00:00').toLocaleDateString(LANGUAGES[i18n.lang].tag, { day: 'numeric', month: 'short', year: 'numeric' });
}

// The engine and the Tauri commands speak English. Turn their messages into
// the current language by matching them against what Rust sends (rust.ts).
const straight = (s: string) => s.replace(/[’‘]/g, "'");
const templates = (Object.entries(RUST) as [Key, string][])
  .map(([k, rustText]) => {
    const parts = straight(rustText).split(/(\{\w+\})/);
    const names: string[] = [];
    const source = parts
      .map((p) => {
        const m = p.match(/^\{(\w+)\}$/);
        if (m) {
          names.push(m[1]);
          return '(.+?)';
        }
        return p.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      })
      .join('');
    return { key: k, re: new RegExp(`^${source}$`, 's'), names };
  });

/** An English message from the backend, in the current language. */
export function tm(message: string | null | undefined): string {
  if (!message) return '';
  const text = straight(message.replace(/^Error: /, ''));
  for (const { key, re, names } of templates) {
    const m = text.match(re);
    if (m) return t(key, Object.fromEntries(names.map((n, i) => [n, m[i + 1]])));
  }
  return message;
}
