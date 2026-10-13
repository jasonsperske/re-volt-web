// Checks the loading bar: it fills while a disc is opened from the file
// picker and again when the page reopens the disc saved in the browser,
// then hides once the toy shop is up. Run `npm run serve` first.
//   node e2e/loading_bar.mjs [disc-basename]
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const disc = process.argv[2] ?? fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const shots = fileURLToPath(new URL('./shots/', import.meta.url));
mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({
  channel: 'chrome', headless: true,
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader', '--ignore-gpu-blocklist'],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));

// Records every value the bar shows (with a timestamp), sampled each frame.
await page.addInitScript(() => {
  window.__bar = [];
  const sample = () => {
    const el = document.getElementById('loading');
    if (el && !el.classList.contains('hidden')) {
      const label = document.getElementById('loadingLabel').textContent;
      if (!window.__bar.at(-1)?.startsWith(label)) window.__bar.push(`${label} @${Math.round(performance.now())}`);
    }
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
});

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures++; };
const pct = (label) => Number(label.match(/(\d+)%/)?.[1]);

async function waitForShop(name) {
  // Screenshot the bar part-way (unless loading was too quick to catch).
  await page.waitForFunction(() => Number(document.getElementById('loading').getAttribute('aria-valuenow')) >= 30
    || window.__app?.frontend_shown(), null, { timeout: 120000 });
  await page.screenshot({ path: `${shots}loading-${name}.png` });
  await page.waitForFunction(() => window.__app?.frontend_shown() && document.getElementById('drop').classList.contains('hidden'), null, { timeout: 120000 });
  const bar = await page.evaluate(() => window.__bar.splice(0));
  const doneAt = await page.evaluate(() => Math.round(performance.now()));
  console.log(`  ${name}: ${bar.join(' | ')} | shop @${doneAt}`);
  const values = bar.map(pct);
  check(values.length >= 5, `${name}: bar updated during loading`);
  check(values.every((v, i) => i === 0 || v >= values[i - 1]), `${name}: bar never goes backwards`);
  // Reading the disc is the slow part, so the bar should move during it,
  // not just jump between steps. (The shop's reads are often too quick to
  // paint more than a frame or two.)
  const disc = new Set(bar.filter((l) => /^(Reading the disc|Opening your saved disc)/.test(l)).map(pct));
  check(disc.size >= 3, `${name}: bar moves while reading the disc (${[...disc].join(', ')}%)`);
  return bar;
}

await page.goto('http://localhost:8080/');
await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your'));
const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
const picked = await waitForShop('picked');
check(picked.some((l) => l.startsWith('Reading the disc')), 'picked: labelled as reading the disc');
console.log('  bytes per step:', await page.evaluate(() => localStorage.getItem('loadBytes')));

// The disc is saved to IndexedDB in the background; wait for it, then reload.
await page.waitForFunction(() => new Promise((resolve) => {
  const open = indexedDB.open('revolt-web', 1);
  open.onsuccess = () => {
    const req = open.result.transaction('disc').objectStore('disc').get('track');
    req.onsuccess = () => resolve(Boolean(req.result?.length));
    req.onerror = () => resolve(false);
  };
  open.onerror = () => resolve(false);
}), null, { timeout: 120000 });
// The browser finishes writing a big save a moment after it's readable;
// a reload inside that moment loses it.
await page.waitForTimeout(1000);
await page.reload();
const cached = await waitForShop('cached');
check(cached.some((l) => l.startsWith('Opening your saved disc')), 'cached: labelled as the saved disc');

check(!errors.length, `no page errors${errors.length ? `: ${errors.join('; ')}` : ''}`);
await browser.close();
process.exit(failures ? 1 : 0);
