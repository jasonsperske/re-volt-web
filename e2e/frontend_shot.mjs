// Screenshots the front end from a few camera positions (development aid).
//   node e2e/frontend_shot.mjs "x,y,z>x,y,z" ...   (game space, Y down)
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const disc = fileURLToPath(new URL('../Re-Volt (USA) (En,Fr,De,Es,It)', import.meta.url));
const shots = fileURLToPath(new URL('./shots/', import.meta.url));
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on('pageerror', (e) => console.error('pageerror', e.message));
await page.goto('http://localhost:8080/');
await page.setInputFiles('#file', [`${disc}.cue`, `${disc} (Track 01).bin`]);
await page.waitForFunction(() => window.__menu?.screen?.title, null, { timeout: 60000 });
const views = process.argv.slice(2);
for (const [i, v] of views.entries()) {
  const [eye, look] = v.split('>').map((p) => p.split(',').map(Number));
  await page.evaluate(([e, l]) => window.__app.frontend_camera(e, l, true), [eye, look]);
  await page.evaluate(() => window.__menu.hide());
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${shots}frontend-${i}.png` });
  console.log(`frontend-${i}.png`, v);
}
await browser.close();
