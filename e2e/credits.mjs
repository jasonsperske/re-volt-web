// Checks the credits: the page on the start screen (before a disc is
// loaded) and the "View Credits" pages in the toy shop, which must all fit
// the 640×480 menu screen. Screenshots go to e2e/shots/. Run `npm run serve`
// first.
//   node e2e/credits.mjs [disc-basename]
import { chromium } from 'playwright-core';
import { mkdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const disc = process.argv[2] ?? fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const shots = fileURLToPath(new URL('./shots/', import.meta.url));
mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({
  channel: 'chrome', headless: true,
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader', '--ignore-gpu-blocklist'],
});
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures++; };
const key = async (k) => { await page.keyboard.press(k); await page.waitForTimeout(80); };
const idle = () => page.waitForFunction(() => window.__menu.panel.state === 'idle', null, { timeout: 20000 });
const source = readFileSync(new URL('../web/credits.js', import.meta.url), 'utf8');
const { CREDITS } = await import(`data:text/javascript,${encodeURIComponent(source)}`);
const everyone = CREDITS.flatMap((p) => p.roles.flatMap((r) => r.names)).filter(Boolean);

// Start screen: the Credits link opens the full roll; Esc closes it.
await page.goto('http://localhost:8080/');
await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your'));
await page.click('#creditsButton');
const sheet = await page.textContent('#creditsBody');
check(await page.isVisible('#credits'), 'start screen: Credits opens the credits page');
check(everyone.every((n) => sheet.includes(n.replaceAll('@', '©'))), `start screen: lists all ${everyone.length} credited names`);
check(sheet.includes('ACCLAIM STUDIOS LONDON') && sheet.includes('THANK YOU FOR PLAYING!'), 'start screen: from "A Game by" to "THANK YOU FOR PLAYING!"');
check(!/Sperske/i.test(sheet), 'start screen: no one but the original credits');
await page.screenshot({ path: `${shots}credits-start.png` });
await key('Escape');
check(!(await page.isVisible('#credits')), 'start screen: Esc closes it');
await page.setViewportSize({ width: 390, height: 760 });
await page.click('#creditsButton');
const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth
  || [...document.querySelectorAll('#credits dd, #credits dt')].some((el) => el.scrollWidth > el.clientWidth + 1));
check(!overflow, 'start screen: fits a phone-width window');
await page.screenshot({ path: `${shots}credits-start-phone.png` });
await page.click('#creditsClose');
await page.setViewportSize({ width: 1280, height: 720 });

// Toy shop: Main menu → View Credits, then page through with Enter.
const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
await page.waitForFunction(() => window.__menu?.open && !window.__menu.screen.title, null, { timeout: 60000 });
await idle();
const labels = await page.evaluate(() => window.__menu.screen.items.map((i) => i.label));
check(labels.includes('View Credits'), `main menu has View Credits (${labels.join(', ')})`);
await key('ArrowDown'); await key('ArrowDown'); await key('Enter');
const seen = [];
const shoot = new Set([1, 3, 13, 14, 18, 27, 28]); // page numbers
for (let i = 0; i < CREDITS.length; i++) {
  await page.waitForFunction((n) => window.__menu.screen?.items.at(-1)?.value?.startsWith(`${n} /`), i + 1, { timeout: 20000 });
  await idle();
  const shown = await page.evaluate(() => {
    const m = window.__menu;
    const { w } = m.size();
    return { title: m.screen.title, text: m.screen.items.map((it) => `${it.label} ${it.text ?? ''}`).join('\n'),
      x: m.panel.x, y: m.panel.y, w, h: m.screen.items.length * 16, titleRight: 108 + m.screen.title.length * 16 + 64 };
  });
  seen.push(shown);
  const tag = shown.title.replace(/\W+/g, '-').toLowerCase();
  if (shoot.has(i + 1)) await page.screenshot({ path: `${shots}credits-${String(i + 1).padStart(2, '0')}-${tag}.png` });
  const fits = shown.x - 20 >= 0 && shown.x + shown.w + 20 <= 640 && shown.y - 24 >= 76 && shown.y + shown.h + 20 <= 480 && shown.titleRight <= 640;
  if (!fits) console.log('  does not fit:', JSON.stringify(shown));
  check(fits, `page ${i + 1} "${shown.title}" fits the screen`);
  await key('Enter');
}
const shopText = seen.map((s) => s.text).join('\n');
check(everyone.every((n) => shopText.includes(n)), `shop: all ${everyone.length} credited names appear across ${seen.length} pages`);
await page.waitForFunction(() => window.__menu.screen && !window.__menu.screen.title, null, { timeout: 20000 });
check(true, 'Done on "The End" returns to the main menu');

check(!errors.length, `no page errors${errors.length ? `: ${errors.join('; ')}` : ''}`);
await browser.close();
process.exit(failures ? 1 : 0);
