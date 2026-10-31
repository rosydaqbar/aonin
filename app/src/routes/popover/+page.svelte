<script lang="ts">
  import { onMount } from 'svelte';
  import Popover from '$lib/Popover.svelte';

  let box: HTMLDivElement;

  onMount(() => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    let stop = () => {};
    (async () => {
      const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window');
      const win = getCurrentWindow();
      // The window is exactly as tall as the popover's content.
      const ro = new ResizeObserver(() => win.setSize(new LogicalSize(360, Math.ceil(box.getBoundingClientRect().height))));
      ro.observe(box);
      const onKey = (e: KeyboardEvent) => e.key === 'Escape' && win.hide();
      window.addEventListener('keydown', onKey);
      stop = () => { ro.disconnect(); window.removeEventListener('keydown', onKey); };
    })();
    return () => stop();
  });
</script>

<svelte:head><title>Aonin</title></svelte:head>
<div class="stage" bind:this={box}><Popover /></div>

<style>
  /* macOS draws the popover in a transparent window so its rounded housing
     shows; Windows 11 rounds the opaque window itself. */
  :global(html:has(.stage)), :global(body:has(.stage)) { background: transparent; overflow: hidden; }
  :global(html.win:has(.stage)) .stage :global(.pop) { border-radius: 0; }
</style>
