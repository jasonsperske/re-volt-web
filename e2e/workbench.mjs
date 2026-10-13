// Checks the workbench: from Select Car, the gear (G) puts the car on the
// shop counter; renaming and tuning are saved to localStorage, used in the
// race, kept after a reload, and Reset to Stock clears them. Screenshots go
// to e2e/shots/. Run `npm run serve` first.
//   node e2e/workbench.mjs [disc-basename]
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
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures++; };
const key = async (k, n = 1) => { for (let i = 0; i < n; i++) { await page.keyboard.press(k); await page.waitForTimeout(60); } };
const menuIs = (title) => page.waitForFunction((t) => {
  const m = window.__menu;
  return m.screen && (m.screen.title ?? 'main') === t && m.panel.state === 'idle';
}, title, { timeout: 30000 });
const value = (label) => page.evaluate((l) => {
  const it = window.__menu.screen.items.find((i) => i.label === l);
  return it && window.__menu.valueText(it);
}, label);
const saved = () => page.evaluate(() => JSON.parse(localStorage.getItem('carTuning') ?? '{}'));

async function open() {
  await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your') || window.__menu?.open);
  if (!(await page.evaluate(() => window.__menu?.open))) {
    const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
    await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
  }
  await menuIs('main');
}
async function toSelectCar() {
  await page.evaluate(() => Object.assign(window.__settings, { car: 'RC', name: 'TESTER', level: 'NHOOD1' }));
  for (const title of ['main', 'Select Race', 'Enter Name']) {
    await menuIs(title);
    await key('Enter');
  }
  await menuIs('Select Car');
}

await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
await open();
await toSelectCar();
check(await page.evaluate(() => window.__menu.screen.corner?.label === 'Workbench'), 'Select Car has the Workbench gear');
await page.screenshot({ path: `${shots}workbench-1-select-car.png` });

// G takes the car to the workbench.
await key('KeyG');
await menuIs('Workbench');
await page.waitForFunction(() => window.__app.fe_car_shown() && window.__app.fe_camera_arrived(), null, { timeout: 10000 });
check(true, 'G opens the Workbench with the car on the counter');
check(await value('Weight') === '100%' && await value('Name') === 'RC Bandit', `stock values shown (Weight ${await value('Weight')}, Name ${await value('Name')})`);

// Weight up to 150%, Spring Stiffness down to 70%.
await key('ArrowDown');
await key('ArrowRight', 5);
await key('ArrowDown');
await key('ArrowLeft', 3);
await page.waitForTimeout(500);
let t = await saved();
check(t.RC?.mass === 1.5 && t.RC?.stiffness === 0.7, `tuning saved to localStorage (${JSON.stringify(t)})`);
await page.screenshot({ path: `${shots}workbench-2-tuning.png` });

// Rename by typing.
await key('ArrowUp', 2);
await key('Enter');
await menuIs('Rename Car');
await key('Backspace', 12);
await page.keyboard.type('Bandito');
await page.screenshot({ path: `${shots}workbench-3-rename.png` });
await key('Enter');
await menuIs('Workbench');
t = await saved();
check(t.RC?.name === 'Bandito' && await value('Name') === 'Bandito', `renamed to "${await value('Name')}" and saved`);

// Done → Select Car shows the new name; race with it.
await page.evaluate(() => { window.__menu.index = window.__menu.screen.items.findIndex((i) => i.label === 'Done'); });
await key('Enter');
await menuIs('Select Car');
check(await value('Car') === 'Bandito', `Select Car shows "${await value('Car')}"`);
for (const title of ['Select Car', 'Select Track', 'Summary']) {
  await menuIs(title);
  await key('Enter');
}
await page.waitForFunction(() => !window.__app.frontend_shown() && window.__app.car_state(), null, { timeout: 60000 });
await page.waitForFunction(() => window.__app.status().includes('Bandito'), null, { timeout: 10000 }).catch(() => {});
check((await page.evaluate(() => window.__app.status())).includes('Bandito'), 'the race uses the renamed, tuned car');

// Once a car is chosen (its box heads for the floor) the gear goes away.
await page.goto('http://localhost:8080/');
await open();
await toSelectCar();
await key('Enter');
await page.waitForTimeout(200);
check(await page.evaluate(() => window.__menu.screen.title === 'Select Car' && !window.__menu.screen.corner), 'gear hidden once the car is chosen');
const box = await page.evaluate(() => { const { scale, ox, oy } = window.__menu.layout(); const r = window.__menu.cornerRect(); return { x: (ox + (r.x + 20) * scale) / devicePixelRatio, y: (oy + (r.y + 15) * scale) / devicePixelRatio }; });
await page.mouse.click(box.x, box.y);
await menuIs('Select Track');
check(!(await page.evaluate(() => window.__app.fe_car_shown() && window.__menu.screen.title === 'Workbench')), 'a click where the gear was does nothing; on to Select Track');

// After a reload (disc from the browser cache) the changes are still there.
await page.waitForTimeout(3000); // the disc is saved to the browser after loading
await page.reload();
await open();
await toSelectCar();
check(await value('Car') === 'Bandito', 'still renamed after a reload');
await key('KeyG');
await menuIs('Workbench');
check(await value('Weight') === '150%' && await value('Spring Stiffness') === '70%', 'tuning kept after a reload');

// Reset to Stock.
await page.evaluate(() => { window.__menu.index = window.__menu.screen.items.findIndex((i) => i.label === 'Reset to Stock'); });
await key('Enter');
await page.waitForFunction(() => {
  const m = window.__menu;
  const name = m.screen?.items.find((i) => i.label === 'Name');
  const weight = m.screen?.items.find((i) => i.label === 'Weight');
  return m.panel.state === 'idle' && name && m.valueText(name) === 'RC Bandit' && m.valueText(weight) === '100%';
}, null, { timeout: 10000 }).catch(() => {});
t = await saved();
check(!t.RC && await value('Name') === 'RC Bandit' && await value('Weight') === '100%', `Reset to Stock clears it (${JSON.stringify(t)})`);
await key('Escape');
await menuIs('Select Car');
check(await value('Car') === 'RC Bandit', 'back on Select Car with the stock name');

check(!errors.length, `no page errors${errors.length ? `: ${errors.join('; ')}` : ''}`);
await browser.close();
process.exit(failures ? 1 : 0);
