<script lang="ts">
  import '$lib/theme.css';
  import { onMount } from 'svelte';
  import { app, followBackend } from '$lib/state.svelte';

  let { children } = $props();
  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  // Tray icon follows the role: active while hosting or connected.
  let active = $derived(!!app.hosting || app.link?.status === 'connected');
  $effect(() => {
    if (!inTauri) return;
    const on = active;
    import('@tauri-apps/api/core').then(({ invoke }) => invoke('set_tray_active', { active: on }));
  });

  onMount(() => {
    document.documentElement.classList.add(navigator.userAgent.includes('Mac') ? 'mac' : 'win');
    const q = new URLSearchParams(location.search);
    const theme = q.get('theme');
    if (theme) document.documentElement.dataset.theme = theme;
    if (inTauri) {
      followBackend();
      return;
    }
    if (q.has('still')) return;
    // Design review: move the demo levels so the grille reads as it does live.
    const id = setInterval(() => {
      for (const c of app.hosting?.clients ?? []) {
        if (c.mark !== 'live' || c.muted) continue;
        c.level = Math.min(0.95, Math.max(0.08, c.level + (Math.random() - 0.5) * 0.18));
        if (Math.random() < 0.01) c.recovered += 1;
      }
    }, 90);
    return () => clearInterval(id);
  });
</script>

{@render children()}
