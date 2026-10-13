// Takes the README screenshots into docs/screenshots/ (run `npm run serve` first).
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const disc = fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const out = fileURLToPath(new URL('../docs/screenshots/', import.meta.url));
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
// A fake gamepad so the race shot can drive (see e2e/smoke.mjs).
await page.addInitScript(() => {
  window.__pad = { axes: [0, 0, 0, 0], buttons: Array(17).fill(0) };
  navigator.getGamepads = () => {
    const btn = (v) => Object.defineProperties(Object.create(GamepadButton.prototype), { value: { value: v }, pressed: { value: v > 0.5 } });
    const pad = Object.defineProperties(Object.create(Gamepad.prototype), {
      id: { value: 'Pad' }, connected: { value: true }, mapping: { value: 'standard' },
      axes: { value: window.__pad.axes.slice() }, buttons: { value: window.__pad.buttons.map(btn) },
    });
    return [pad, null, null, null];
  };
});
await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
const menuIs = (t) => page.waitForFunction((t) => (window.__menu.screen?.title ?? (window.__menu.screen ? 'main' : null)) === t
  && window.__menu.panel.state === 'idle' && window.__app.fe_camera_arrived(), t, { timeout: 60000 });
const shot = async (name) => { await page.waitForTimeout(500); await page.screenshot({ path: `${out}${name}.jpg`, type: 'jpeg', quality: 82 }); console.log(name); };

await menuIs('main');
await page.waitForFunction(() => window.__app.race_state()?.racers > 0, null, { timeout: 60000 });
await shot('shop-main-menu');

await page.evaluate(() => Object.assign(window.__settings, { level: 'TOYLITE', car: 'TC4', name: 'PLAYER', opponents: 5 }));
for (const t of ['main', 'Select Race', 'Enter Name']) { await menuIs(t); await page.keyboard.press('Enter'); }
await menuIs('Select Car');
await page.waitForTimeout(1500); // let the box slide out
await shot('shop-select-car');
await page.keyboard.press('Enter');
for (const t of ['Select Track', 'Summary']) { await menuIs(t); await page.keyboard.press('Enter'); }
await page.waitForFunction(() => !window.__app.frontend_shown() && window.__app.race_state()?.countdown === 0, null, { timeout: 60000 });
await page.addStyleTag({ content: '#panel, #status { display: none !important; }' });
// Shortly after GO, with the field still bunched up on the start straight.
await page.evaluate(() => { window.__pad.buttons[7] = 1; });
await page.waitForFunction(() => window.__app.car_state().speed_mph > 9, null, { timeout: 20000 }).catch(() => {});
await shot('race');
await browser.close();
