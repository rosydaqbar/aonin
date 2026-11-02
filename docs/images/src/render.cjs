// Renders how-it-works.html to ../how-it-works.png (still, 2x) and
// ../how-it-works.webp (looping animation). Needs playwright-core with
// Microsoft Edge or Chrome installed, and ffmpeg built with libwebp.
//   node docs/images/src/render.cjs
const { chromium } = require('playwright-core');
const { execFileSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const here = __dirname;
const out = path.join(here, '..');
const page = 'file:///' + path.join(here, 'how-it-works.html').replace(/\\/g, '/');
const FRAMES = 80; // 3.2 s at 25 fps
const WIDTH = 1280; // the animation, scaled from the 1600 x 900 artboard

(async () => {
  const channel = process.env.PW_CHANNEL || (process.platform === 'win32' ? 'msedge' : 'chrome');
  const browser = await chromium.launch({ channel });

  const still = await browser.newPage({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 2 });
  await still.goto(page);
  await still.evaluate(() => document.fonts.ready);
  await still.evaluate(() => window.render(0.3));
  await still.screenshot({ path: path.join(out, 'how-it-works.png') });
  const svg = await still.evaluate(() => document.getElementById('art').outerHTML);
  fs.writeFileSync(path.join(out, 'how-it-works.svg'), svg);

  const scale = WIDTH / 1600;
  const anim = await browser.newPage({ viewport: { width: WIDTH, height: Math.round(900 * scale) } });
  await anim.goto(page);
  await anim.evaluate((s) => {
    const svg = document.getElementById('art');
    svg.setAttribute('width', 1600 * s);
    svg.setAttribute('height', 900 * s);
  }, scale);
  await anim.evaluate(() => document.fonts.ready);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aonin-frames-'));
  for (let i = 0; i < FRAMES; i++) {
    await anim.evaluate((t) => window.render(t), i / FRAMES);
    await anim.screenshot({ path: path.join(dir, `f${String(i).padStart(3, '0')}.png`) });
  }
  await browser.close();

  execFileSync('ffmpeg', [
    '-y', '-loglevel', 'error', '-framerate', '25', '-i', path.join(dir, 'f%03d.png'),
    '-c:v', 'libwebp_anim', '-lossless', '0', '-q:v', '78', '-compression_level', '6', '-loop', '0',
    path.join(out, 'how-it-works.webp'),
  ]);
  fs.rmSync(dir, { recursive: true, force: true });
  for (const f of ['how-it-works.png', 'how-it-works.webp', 'how-it-works.svg']) {
    console.log(f, Math.round(fs.statSync(path.join(out, f)).size / 1024) + ' KB');
  }
})();
