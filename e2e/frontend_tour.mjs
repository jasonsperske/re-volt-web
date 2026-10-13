// Walks the toy-shop front end with the keyboard and screenshots each
// screen (development aid; run `npm run serve` first).
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const disc = fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const shots = fileURLToPath(new URL('./shots/', import.meta.url));
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
await page.setInputFiles('#file', [`${disc}.cue`, `${disc} (Track 01).bin`, `${disc} (Track 02).bin`]);

const title = () => page.evaluate(() => window.__menu.screen ? (window.__menu.screen.title ?? '(main)') : null);
const settle = async () => {
  await page.waitForFunction(() => window.__app.fe_camera_arrived() && window.__menu.panel.state === 'idle', null, { timeout: 30000 }).catch(() => {});
  await page.waitForTimeout(400);
};
async function shot(name) {
  await settle();
  await page.screenshot({ path: `${shots}fe-${name}.png` });
  console.log(`fe-${name}.png  menu: ${await title()}`);
}
const key = async (k, n = 1) => { for (let i = 0; i < n; i++) { await page.keyboard.press(k); await page.waitForTimeout(120); } };

await page.waitForFunction(() => window.__menu?.open, null, { timeout: 60000 });
await shot('1-main');
await key('Enter'); await shot('2-select-race');
await key('Enter'); await shot('3-enter-name');
await key('ArrowRight', 9); await key('Enter');          // J
await key('ArrowLeft', 9 + 1); await key('ArrowLeft', 0); // back toward the end
await page.evaluate(() => { window.__app.fe_set_wheel(29); });
await page.waitForTimeout(200);
await page.evaluate(() => {
  // jump the wheel to END through the item handlers
  const it = window.__menu.screen.items[0];
  for (let i = 0; i < 30 && !it.value.includes('END'); i++) it.right();
});
await shot('4-name-entered');
await key('Enter'); await shot('5-select-car');
await key('ArrowRight', 2); await shot('6-car-pulled-out');
await key('Enter');
await page.waitForFunction(() => window.__app.fe_car_shown(), null, { timeout: 20000 }).catch(() => {});
await page.waitForTimeout(700);
await page.screenshot({ path: `${shots}fe-6b-car-dropped.png` });
console.log('fe-6b-car-dropped.png');
await page.waitForFunction(() => window.__menu.screen?.title === 'Select Track', null, { timeout: 20000 }).catch(() => {});
await shot('7-select-track');
await key('ArrowRight'); await shot('8-track-changed');
await key('Enter'); await shot('9-summary');
await key('Escape'); await settle(); await key('Escape'); await settle(); await key('Escape'); await settle(); await key('Escape');
await page.waitForTimeout(800);
await key('Escape'); await settle();
console.log('back to:', await title());
await key('ArrowDown'); await key('Enter'); await shot('10-options');
console.log(errors.length ? `errors: ${errors.join(' | ')}` : 'no page errors');
await browser.close();
