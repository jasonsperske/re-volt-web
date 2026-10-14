// Checks Options → Controller Settings: rebinding by key and by gamepad
// button (taking the key off any other action), driving with the new
// mapping, saving across a reload, and Reset to Default. Screenshot in
// e2e/shots/. Run `npm run serve` first.
//   node e2e/controls.mjs [disc-basename]
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
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));

// A fake standard-mapping gamepad (as in smoke.mjs), driven via window.__pad.
await page.addInitScript(() => {
  window.__pad = { axes: [0, 0, 0, 0], buttons: Array(17).fill(0) };
  navigator.getGamepads = () => {
    const btn = (v) => Object.defineProperties(Object.create(GamepadButton.prototype),
      { value: { value: v }, pressed: { value: v > 0.5 } });
    const pad = Object.defineProperties(Object.create(Gamepad.prototype), {
      id: { value: 'Test Pad (STANDARD GAMEPAD)' }, connected: { value: true }, mapping: { value: 'standard' },
      axes: { value: window.__pad.axes.slice() }, buttons: { value: window.__pad.buttons.map(btn) },
    });
    return [pad, null, null, null];
  };
});

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures++; };
const menuIs = (title) => page.waitForFunction((t) => {
  const m = window.__menu;
  return m.screen && (m.screen.title ?? 'main') === t && m.panel.state === 'idle';
}, title, { timeout: 30000 });
const bindings = () => page.evaluate(() => window.__app.bindings());
const saved = () => page.evaluate(() => JSON.parse(localStorage.getItem('controls') || 'null'));
const row = (label) => page.evaluate((l) => window.__menu.screen.items.findIndex((i) => i.label === l), label);
async function pick(label) {
  await page.evaluate((i) => { window.__menu.index = i; }, await row(label));
  await page.keyboard.press('Enter');
}
async function open() {
  await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your') || window.__menu?.open);
  if (!(await page.evaluate(() => window.__menu?.open))) {
    const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
    await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
  }
  await menuIs('main');
}
async function toControllerSettings() {
  await pick('Options');
  await menuIs('Options');
  await pick('Controller Settings');
  await menuIs('Controller Settings');
}

await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
await open();
await toControllerSettings();
check(await page.evaluate(() => window.__menu.screen.items.find((i) => i.label === 'Fire')?.value === 'Space, Enter | B, RB'),
  'Options → Controller Settings lists each action with its keys and buttons');

// Fire → K.
await pick('Fire');
await menuIs('Fire');
await page.keyboard.press('KeyK');
await menuIs('Controller Settings');
let b = await bindings();
check(b.fire.keys.join() === 'KeyK' && (await saved())?.fire.keys.join() === 'KeyK', 'a key press rebinds Fire to K and saves it');

// Accelerate → gamepad RB (button 5), which Fire had.
await pick('Accelerate');
await menuIs('Accelerate');
await page.evaluate(() => { window.__pad.buttons[5] = 1; });
await menuIs('Controller Settings');
await page.evaluate(() => { window.__pad.buttons[5] = 0; });
b = await bindings();
check(b.accelerate.pad.join() === '5' && b.fire.pad.join() === '1', `a gamepad press rebinds Accelerate to RB and takes it off Fire (${b.accelerate.pad} / ${b.fire.pad})`);
await page.waitForTimeout(300);
check(await page.evaluate(() => window.__menu.screen.title === 'Controller Settings'), "the button that was just bound doesn't also press the menu");

// Accelerate → I on the keyboard; Flip → W takes W off nothing else now.
await pick('Accelerate');
await menuIs('Accelerate');
await page.keyboard.press('KeyI');
await menuIs('Controller Settings');
await pick('Restart Race');
await menuIs('Restart Race');
await page.keyboard.press('KeyM'); // reserved: ignored
await page.keyboard.press('Escape');
await menuIs('Controller Settings');
b = await bindings();
check(b.accelerate.keys.join() === 'KeyI' && b.restart.keys.join() === 'KeyR', 'M is reserved and Esc cancels');
await page.screenshot({ path: `${shots}controls.png` });

// Race with I as the accelerator.
await page.keyboard.press('Escape');
await menuIs('Options');
await page.keyboard.press('Escape');
await menuIs('main');
await page.evaluate(() => Object.assign(window.__settings, { level: 'NHOOD1', car: 'RC', name: 'TESTER', opponents: 0 }));
for (const t of ['main', 'Select Race', 'Enter Name', 'Select Car', 'Select Track', 'Summary']) {
  await menuIs(t);
  await page.keyboard.press('Enter');
}
await page.waitForFunction(() => !window.__app.frontend_shown() && window.__app.race_state()?.countdown <= 0, null, { timeout: 60000 });
const start = await page.evaluate(() => window.__app.car_state());
await page.keyboard.down('KeyW');
await page.waitForTimeout(1500);
await page.keyboard.up('KeyW');
const afterW = await page.evaluate(() => window.__app.car_state());
await page.keyboard.down('KeyI');
await page.waitForTimeout(2000);
await page.keyboard.up('KeyI');
const afterI = await page.evaluate(() => window.__app.car_state());
const dist = (a, c) => Math.hypot(a.x - c.x, a.z - c.z);
check(dist(start, afterW) < 30 && dist(afterW, afterI) > 200, `W no longer accelerates (${dist(start, afterW).toFixed(0)}), I does (${dist(afterW, afterI).toFixed(0)})`);
check((await page.textContent('#help')).includes('I accelerate'), 'the help line shows the new keys');

// Kept after a reload, then Reset to Default.
await page.reload();
await open();
b = await bindings();
check(b.accelerate.keys.join() === 'KeyI' && b.fire.keys.join() === 'KeyK', 'the mapping is kept after a reload');
await toControllerSettings();
await pick('Reset to Default');
await page.waitForTimeout(800);
b = await bindings();
check(b.accelerate.keys.join() === 'KeyW,ArrowUp' && b.fire.pad.join() === '1,5' && !(await saved()), 'Reset to Default restores the defaults');

check(!errors.length, `no page errors${errors.length ? `: ${errors.join('; ')}` : ''}`);
await browser.close();
process.exit(failures ? 1 : 0);
