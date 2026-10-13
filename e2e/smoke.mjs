// Headless browser smoke test using the system Chrome. Serves nothing
// itself: run `npm run serve` first. Opens the disc through the file input,
// drives with a simulated gamepad, and saves screenshots to e2e/shots/.
//
//   node e2e/smoke.mjs [disc-basename]
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

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
page.on('console', (m) => m.type() === 'error' && !m.text().includes('favicon') && errors.push(m.text()));

// A fake standard-mapping gamepad built on the real prototypes, so the
// engine's `instanceof Gamepad` checks pass. Driven through window.__pad.
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
const pad = (axes, buttons = {}) => page.evaluate(([a, b]) => {
  window.__pad.axes = a;
  window.__pad.buttons = window.__pad.buttons.map((_, i) => b[i] ?? 0);
}, [axes, buttons]);
const car = () => page.evaluate(() => window.__app.car_state());
// Polls car state until `pred(state, start)` holds; returns the state or null.
// Headless software rendering runs at a few fps, so tests wait on outcomes
// rather than fixed delays.
const until = (pred, start, timeout = 20000) => page
  .waitForFunction(([src, s0]) => {
    const st = window.__app.car_state();
    return st && new Function('s', 's0', `return (${src})(s, s0)`)(st, s0) ? st : null;
  }, [pred.toString(), start], { timeout, polling: 100 })
  .then((h) => h.jsonValue(), () => null);
const check = (ok, msg) => { if (!ok) { console.error('FAIL:', msg); process.exitCode = 1; } else console.log('ok:', msg); };

await page.goto('http://localhost:8080/');
await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your'));
const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
// Opening the disc lands in the toy-shop front end on the main menu.
const menuIs = (title) => page.waitForFunction((t) => {
  const m = window.__menu;
  return m.screen && (m.screen.title ?? 'main') === t && m.panel.state === 'idle';
}, title, { timeout: 30000 });
await menuIs('main');
await page.evaluate(() => localStorage.clear());
const shopShown = await page.evaluate(() => window.__app.frontend_shown());
await page.keyboard.press('ArrowDown');
const down = await page.evaluate(() => window.__menu.index);
await page.keyboard.press('ArrowUp');
const up = await page.evaluate(() => window.__menu.index);
check(shopShown && down === 1 && up === 0, `toy-shop main menu with keyboard navigation (rows ${down} → ${up})`);
await page.waitForFunction(() => window.__app?.race_state()?.racers === 4, null, { timeout: 60000 }).catch(() => {});

// Main → Select Race → Enter Name → Select Car → Select Track → Summary → race.
async function raceFromShop(level, carId) {
  await page.evaluate(([l, c]) => Object.assign(window.__settings, { level: l, car: c, name: 'TESTER' }), [level, carId]);
  for (const title of ['main', 'Select Race', 'Enter Name', 'Select Car', 'Select Track', 'Summary']) {
    await menuIs(title);
    await page.keyboard.press('Enter');
  }
  await page.waitForFunction(() => !window.__menu.open && !window.__app.frontend_shown() && window.__app.car_state(),
    null, { timeout: 60000 });
}
await raceFromShop('NHOOD1', 'RC');
check(true, 'raced from the shop: Start Race → Single Race → name → car → track → summary');

// Race start: countdown banner, car held until GO.
await pad([0, 0, 0, 0], { 7: 1 }); // try to jump the start
const countdown = await page.waitForFunction(() => /^[123]$/.test(document.getElementById('banner').textContent)
  && document.getElementById('banner').textContent, null, { timeout: 10000 }).then((h) => h.jsonValue(), () => null);
const h0 = await car();
await page.waitForTimeout(1000);
const h1 = await car();
const crept = Math.hypot(h1.x - h0.x, h1.z - h0.z);
check(countdown && crept < 5, `countdown "${countdown}" holds the car despite throttle (moved ${crept.toFixed(1)} units)`);
check(/LAP 1\/3/.test(await page.textContent('#hud')), `HUD shows ${await page.textContent('#hud .big')}`);
await pad([0, 0, 0, 0]);
await page.waitForFunction(() => window.__app.race_state().countdown === 0, null, { timeout: 20000 });
const field = await page.evaluate(() => window.__app.race_state());
check(field.racers === 4 && /\d(st|nd|rd|th) \/ 4/.test(await page.textContent('#hud')),
  `grid of ${field.racers} cars, HUD shows ${await page.textContent('#hud .big')}`);
const aiMoving = await page.waitForFunction(() => {
  const s = Array.from(window.__app.racer_speeds());
  return s.slice(1).filter((v) => v > 5).length >= 2 && s;
}, null, { timeout: 20000 }).then((h) => h.jsonValue(), () => null);
check(aiMoving, `AI opponents drive off the grid (mph ${aiMoving ? aiMoving.slice(1).map((v) => v.toFixed(0)).join(', ') : 'none'})`);
await page.screenshot({ path: `${shots}race-start.png` });
await page.waitForTimeout(1000); // settle on the suspension
const start = await car();

// Music: the click is the user gesture that unlocks audio; Toys in the Hood
// plays REDBOOK 4..9, i.e. CD tracks 6..11.
await page.mouse.click(640, 400);
const music = await page.waitForFunction(() => {
  const m = window.__music;
  return m.sources?.length > 0 && { playlist: m.playlist, tracks: m.tracks.size, scheduled: m.sources.length };
}, null, { timeout: 15000 }).then((h) => h.jsonValue(), () => null);
check(music && music.tracks === 14 && music.playlist.join() === '6,7,8,9,10,11',
  `music streams CD tracks ${music ? music.playlist.join(',') : '(none)'} of ${music?.tracks} audio tracks`);
check(start.upright && start.speed_mph < 1, `car settles on the start line (${start.speed_mph.toFixed(2)} mph)`);
await page.screenshot({ path: `${shots}start.png` });

await pad([0, 0, 0, 0], { 7: 1 }); // right trigger
const moving = await until((s, s0) => Math.hypot(s.x - s0.x, s.z - s0.z) > 500, start) ?? await car();
const moved = Math.hypot(moving.x - start.x, moving.z - start.z);
check(moved > 500 && moving.speed_mph > 10, `RT accelerates (${moved.toFixed(0)} units, ${moving.speed_mph.toFixed(1)} mph)`);
await page.screenshot({ path: `${shots}driving.png` });
const engine = await page.evaluate(() => {
  const l = window.__sfx.loops.get('engine');
  return l?.src ? { path: l.path, rate: l.src.playbackRate.value } : null;
});
check(engine && engine.rate > 0.6, `engine sound ${engine?.path} pitched up to ${engine?.rate.toFixed(2)}x while driving`);

await pad([1, 0, 0, 0], { 7: 0.6 }); // steer right
await page.waitForTimeout(1000);
await page.screenshot({ path: `${shots}turning.png` });
await pad([0, 0, 0, 0], { 6: 1 }); // brake
await page.waitForTimeout(1500);
const braked = await car();
check(braked.upright, 'still upright after turning and braking');

await pad([0, 0, 0, 0], { 3: 1 }); // Y = reset
const reset = await until((s, s0) => Math.hypot(s.x - s0.x, s.z - s0.z) < 30, start);
await pad([0, 0, 0, 0]);
check(reset !== null, 'Y resets the car to the start');

// Pickups appear 5 s into the race: 2N+5 = 13 for 4 cars (NHOOD1 has 27 points).
const pickups = await page.waitForFunction(() => window.__app.live_pickups() > 0 && window.__app.live_pickups(), null, { timeout: 30000 })
  .then((h) => h.jsonValue(), () => 0);
check(pickups === 13, `${pickups} pickups on the track`);
await page.screenshot({ path: `${shots}pickups.png` });

// Weapons: hand the player a firework pack, check the HUD, fire one with Space.
await page.evaluate(() => window.__app.give_weapon(2));
const hudWeapon = await page.waitForFunction(() => document.querySelector('#hud .weapon')?.textContent, null, { timeout: 5000 })
  .then((h) => h.jsonValue(), () => null);
await page.keyboard.press('Space');
const usesLeft = await page.waitForFunction(() => window.__app.race_state().uses === 2 && 2, null, { timeout: 5000 })
  .then((h) => h.jsonValue(), () => null);
check(hudWeapon === 'Firework Pack ×3' && usesLeft === 2, `HUD shows "${hudWeapon}", Space fires one (${usesLeft} left)`);
await page.waitForTimeout(300);
await page.screenshot({ path: `${shots}firework.png` });

// Pause menu: Esc pauses (nothing moves), Resume continues.
await pad([0, 0, 0, 0], { 7: 1 });
await page.waitForTimeout(500);
await page.keyboard.press('Escape');
const p0 = await car();
await page.waitForTimeout(800);
const p1 = await car();
const pausedTitle = await page.evaluate(() => window.__menu.screen?.title);
check(pausedTitle === 'Paused' && Math.hypot(p1.x - p0.x, p1.z - p0.z) < 1, `Esc pauses the race (${pausedTitle})`);
await page.keyboard.press('Enter'); // Resume
await pad([0, 0, 0, 0]);
check(!(await page.evaluate(() => window.__menu.open)), 'Enter on Resume closes the menu');

// Quit back to the shop and race other tracks and cars.
async function raceOn(level, carId) {
  await page.keyboard.press('Escape');
  await menuIs('Paused');
  await page.evaluate(() => { const m = window.__menu; m.index = m.screen.items.findIndex((i) => i.label === 'Quit'); });
  await page.keyboard.press('Enter');
  await raceFromShop(level, carId);
}
for (const [level, carId] of [['TOYLITE', 'TC4'], ['MUSE2', 'DINO'], ['GARDEN1', 'ROTOR']]) {
  await raceOn(level, carId);
  await page.waitForFunction((l) => document.getElementById('status').textContent.includes(`(${l})`), level, { timeout: 60000 });
  const s0 = await car();
  await pad([0, 0, 0, 0], { 7: 1 });
  const s = await until((s, s0) => s.speed_mph > 8, s0) ?? await car();
  await pad([0, 0, 0, 0]);
  check(s.upright && s.speed_mph > 8, `${level} with ${carId} drives (${s.speed_mph.toFixed(1)} mph)`);
  await page.screenshot({ path: `${shots}${level.toLowerCase()}.png` });
}

console.log('status:', await page.textContent('#status'));
check(errors.length === 0, `no page errors${errors.length ? ': ' + errors.join(' | ') : ''}`);
await browser.close();
