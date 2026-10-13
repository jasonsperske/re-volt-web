// Checks VR mode with a stand-in WebXR device (no headset needed): Enter VR
// starts a session that draws both eyes (side by side on the canvas here),
// the VR controllers drive the car, and Y pauses and leaves VR. The
// screenshot of the two eyes goes to e2e/shots/. Run `npm run serve` first.
//   node e2e/vr.mjs [disc-basename]
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
const page = await browser.newPage({ viewport: { width: 1280, height: 640 } });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => m.type() === 'error' && !m.text().includes('favicon') && errors.push(m.text()));

// The stand-in device: a session with two eyes 64 mm apart, xr-standard
// controllers driven through window.__xr, and a layer that draws to the
// canvas (left eye on the left half).
await page.addInitScript(() => {
  window.__xr = { buttons: { left: Array(6).fill(0), right: Array(6).fill(0) }, axes: { left: [0, 0, 0, 0], right: [0, 0, 0, 0] }, frames: 0 };
  WebGL2RenderingContext.prototype.makeXRCompatible = async () => {};
  window.XRWebGLLayer = class {
    constructor(session, gl) { this.gl = gl; this.framebuffer = null; }
    getViewport(view) {
      const w = this.gl.drawingBufferWidth / 2, h = this.gl.drawingBufferHeight;
      return { x: view.eye === 'left' ? 0 : w, y: 0, width: w, height: h };
    }
  };
  const perspective = (fovy, aspect, n, f) => {
    const t = 1 / Math.tan(fovy / 2);
    return new Float32Array([t / aspect, 0, 0, 0, 0, t, 0, 0, 0, 0, (f + n) / (n - f), -1, 0, 0, (2 * f * n) / (n - f), 0]);
  };
  const views = (gl) => ['left', 'right'].map((eye) => ({
    eye,
    projectionMatrix: perspective(1.5, gl.drawingBufferWidth / 2 / gl.drawingBufferHeight, 0.05, 2000),
    transform: { inverse: { matrix: new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, eye === 'left' ? 0.032 : -0.032, 0, 0, 1]) } },
  }));
  const pad = (hand) => ({
    get buttons() { return window.__xr.buttons[hand].map((v) => ({ value: v, pressed: v > 0.5 })); },
    get axes() { return window.__xr.axes[hand]; },
  });
  const session = {
    inputSources: [{ handedness: 'left', gamepad: pad('left') }, { handedness: 'right', gamepad: pad('right') }],
    renderState: {},
    listeners: {},
    updateRenderState(s) { Object.assign(this.renderState, s); },
    async requestReferenceSpace() { return {}; },
    requestAnimationFrame(cb) {
      return requestAnimationFrame((t) => {
        if (this.ended) return;
        window.__xr.frames++;
        cb(t, { getViewerPose: () => ({ views: views(this.renderState.baseLayer.gl) }) });
      });
    },
    addEventListener(type, fn) { (this.listeners[type] ??= []).push(fn); },
    end() { this.ended = true; (this.listeners.end ?? []).forEach((f) => f()); return Promise.resolve(); },
  };
  Object.defineProperty(navigator, 'xr', {
    value: { isSessionSupported: async (mode) => mode === 'immersive-vr', requestSession: async () => { session.ended = false; return session; } },
  });
});

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures++; };
const menuIs = (title) => page.waitForFunction((t) => {
  const m = window.__menu;
  return m.screen && (m.screen.title ?? 'main') === t && m.panel.state === 'idle';
}, title, { timeout: 30000 });

await page.goto('http://localhost:8080/');
await page.evaluate(() => localStorage.clear());
await page.waitForFunction(() => document.getElementById('status').textContent.includes('Open your'));
const tracks = Array.from({ length: 15 }, (_, i) => `${disc} (Track ${String(i + 1).padStart(2, '0')}).bin`);
await page.setInputFiles('#file', [`${disc}.cue`, ...tracks]);
await page.evaluate(() => Object.assign(window.__settings, { level: 'NHOOD1', car: 'RC', name: 'TESTER', opponents: 3 }));
for (const t of ['main', 'Select Race', 'Enter Name', 'Select Car', 'Select Track', 'Summary']) {
  await menuIs(t);
  await page.keyboard.press('Enter');
}
await page.waitForFunction(() => !window.__app.frontend_shown() && window.__app.car_state(), null, { timeout: 60000 });

check(await page.isVisible('#vr'), 'Enter VR button shown when the browser supports immersive VR');
await page.click('#vr');
await page.waitForFunction(() => window.__xrMode.active && window.__xr.frames > 30, null, { timeout: 10000 });
check(true, 'Enter VR starts a session and the headset loop draws frames');
// Restart the race (R) to see a countdown in VR.
await page.keyboard.press('KeyR');
await page.waitForFunction(() => window.__app.race_state()?.countdown > 0 && window.__app.race_state()?.countdown < 2.5, null, { timeout: 5000 }).catch(() => {});
await page.waitForTimeout(300);
await page.screenshot({ path: `${shots}vr-countdown.png` });
check(/^[123]$/.test(await page.evaluate(() => JSON.parse(window.__xrMode.lastHud || '{}').banner)), 'the VR HUD shows the countdown');

// Right trigger accelerates.
await page.waitForFunction(() => window.__app.race_state()?.countdown <= 0, null, { timeout: 10000 });
const before = await page.evaluate(() => window.__app.car_state());
await page.evaluate(() => { window.__xr.buttons.right[0] = 1; });
await page.waitForTimeout(2500);
const after = await page.evaluate(() => window.__app.car_state());
await page.screenshot({ path: `${shots}vr-driving.png` });
const moved = Math.hypot(after.x - before.x, after.z - before.z);
check(moved > 200, `right trigger drives the car (${moved.toFixed(0)} units)`);
await page.evaluate(() => { window.__xr.buttons.right[0] = 0; });

// X changes the camera.
const cam = await page.evaluate(() => window.__app.camera_mode());
await page.evaluate(() => { window.__xr.buttons.left[4] = 1; });
await page.waitForTimeout(200);
await page.evaluate(() => { window.__xr.buttons.left[4] = 0; });
await page.waitForTimeout(200);
check(await page.evaluate(() => window.__app.camera_mode()) === cam + 1, 'X changes the camera');

// Y pauses, which leaves VR for the menu.
await page.evaluate(() => { window.__xr.buttons.left[5] = 1; });
await page.waitForFunction(() => !window.__xrMode.active, null, { timeout: 5000 }).catch(() => {});
await page.evaluate(() => { window.__xr.buttons.left[5] = 0; });
check(!(await page.evaluate(() => window.__xrMode.active)) && await page.evaluate(() => window.__menu.open && window.__menu.screen.title === 'Paused'),
  'Y pauses and leaves VR');
await page.waitForTimeout(500);
check(await page.evaluate(() => window.__menu.open), 'the flat-screen game carries on (pause menu shown)');

check(!errors.length, `no page errors${errors.length ? `: ${errors.join('; ')}` : ''}`);
await browser.close();
process.exit(failures ? 1 : 0);
