// Screenshots the name wheel at a few positions with the menu label, to
// check the letter under the pointer matches (development aid).
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const disc = fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const shots = fileURLToPath(new URL('./shots/', import.meta.url));
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
await page.setInputFiles('#file', [`${disc}.cue`, `${disc} (Track 01).bin`]);
const idle = (t) => page.waitForFunction((t) => (window.__menu.screen?.title ?? 'main') === t && window.__menu.panel.state === 'idle' && window.__app.fe_camera_arrived(), t, { timeout: 30000 })
  .catch(async (e) => { console.log('state:', await page.evaluate(() => JSON.stringify({ title: window.__menu.screen?.title, panel: window.__menu.panel.state, arrived: window.__app.fe_camera_arrived() }))); throw e; });
await idle('main');
// Wait for the background race load to finish before navigating.
await page.waitForFunction(() => window.__app.race_state()?.racers > 0, null, { timeout: 60000 });
await page.keyboard.press('Enter');
await idle('Select Race'); await page.keyboard.press('Enter');
await idle('Enter Name');
let at = 0;
for (const target of [0, 1, 2, 7, 25, 28, 29]) {
  const key = target >= at ? 'ArrowRight' : 'ArrowLeft';
  while (at !== target) { await page.keyboard.press(key); at += key === 'ArrowRight' ? 1 : -1; await page.waitForTimeout(60); }
  await page.waitForTimeout(1500); // let the wheel ease in
  const label = await page.evaluate(() => window.__menu.screen.items[0].value);
  await page.screenshot({ path: `${shots}wheel-${String(target).padStart(2, '0')}.png`, clip: { x: 440, y: 120, width: 400, height: 520 } });
  console.log(`wheel-${String(target).padStart(2, '0')}.png  label "${label}"`);
}
await browser.close();
