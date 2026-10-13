import init, { App } from './pkg/revolt_web.js';
import { CdMusic, findAudioTracks } from './music.js';
import { SoundFx } from './sfx.js';
import { Menu } from './menu.js';
import { CREDITS } from './credits.js';

const $ = (id) => document.getElementById(id);
const canvas = $('view');
const status = $('status');

let message = 'Loading engine…';
const setMessage = (m) => { message = m; status.textContent = m; };

await init();
const app = new App(canvas);
const music = new CdMusic();
music.setEnabled(localStorageGet('music') !== 'off');
const sfx = new SoundFx(app, () => music.ctx);
sfx.setEnabled(localStorageGet('sound') !== 'off');
const menu = new Menu($('menu'), (path) => sfx.play(path, 0.6));
// Menus move the shop camera to their node while the front end is shown.
menu.onNode = (node) => { if (app.frontend_shown()) app.fe_goto(node, 0.85); };
window.__app = app; // for e2e tests and console debugging
window.__music = music;
window.__sfx = sfx;
window.__menu = menu;

// A level's `REDBOOK a b` plays CD tracks a+2..b+2 (play_redbook_range
// 0x4892e0 in REVOLT.EXE; docs/weapons.md).
const REDBOOK_TRACK_OFFSET = 2;
setMessage('Open your Re-Volt disc to begin.');

// ---- Race settings ------------------------------------------------------

const settings = {
  mode: localStorageGet('mode') ?? 'single', // single | trial | stunt
  name: localStorageGet('name') ?? '',
  level: localStorageGet('level') ?? 'NHOOD1',
  car: localStorageGet('car') ?? 'RC',
  opponents: Number(localStorageGet('opponents') ?? 3),
  laps: Number(localStorageGet('laps') ?? 3),
  pickups: localStorageGet('pickups') !== 'false',
};
// What the engine currently has loaded, so starting a race only reloads changes.
const applied = { level: null, car: null, opponents: null, laps: null, pickups: null };
let catalog = { levels: [], cars: [] };
window.__settings = settings; // for e2e tests
let S = []; // the game's strings (STRINGS/ENGLISH.TXT), by id
const str = (id, fallback) => S[id] || fallback;

// Battle arenas need Battle Tag, which this version doesn't have.
const ARENAS = new Set(['BOT_BAT', 'MARKAR', 'MUSE_BAT', 'NHOOD1_BATTLE', 'STUNTS']);
// The game's car order (docs/frontend.md; it decides which box leaves the shelf).
const CAR_ORDER = ['RC', 'MITE', 'PHAT', 'MOSS', 'MUD', 'BEATALL', 'VOLKEN', 'TC6', 'DINO', 'CANDY', 'GENCAR', 'TC4', 'MOUSE',
  'FLAG', 'TC2', 'R5', 'TC5', 'SGT', 'TC3', 'ADEON', 'FONE', 'TC1', 'ROTOR', 'COUGAR', 'SUGO', 'TOYECA', 'AMW', 'PANGA'];
const raceTracks = () => catalog.levels.filter((l) => !ARENAS.has(l.id));
const orderedCars = () => [...catalog.cars].sort((a, b) =>
  (CAR_ORDER.indexOf(a.id) + 1 || 99) - (CAR_ORDER.indexOf(b.id) + 1 || 99));
const nameOf = (list, id) => list.find((e) => e.id === id)?.name ?? id;
// A car's name, as renamed on the workbench.
const carName = (id) => tunings[id]?.name || nameOf(catalog.cars, id);

// What the chosen mode actually races with.
function effective() {
  switch (settings.mode) {
    case 'trial': return { ...settings, opponents: 0, pickups: false };
    case 'stunt': return { ...settings, level: 'STUNTS', opponents: 0, pickups: false };
    default: return { ...settings };
  }
}

async function applySettings() {
  for (const k of Object.keys(settings)) localStorageSet(k, String(settings[k]));
  const want = effective();
  if (applied.level !== want.level) {
    setMessage(`Loading ${nameOf(catalog.levels, want.level)}…`);
    await app.load_level(want.level);
    applied.level = want.level;
  }
  const carChanged = applied.car !== want.car;
  if (carChanged) {
    await app.load_car(want.car);
    applied.car = want.car;
  }
  if (carChanged || applied.opponents !== want.opponents) {
    setMessage('Loading opponents…');
    await app.set_opponents(want.opponents);
    applied.opponents = want.opponents;
  }
  if (applied.pickups !== want.pickups) {
    app.set_pickups(want.pickups);
    applied.pickups = want.pickups;
  }
  app.set_laps(want.laps); // also restarts from the grid
  applied.laps = want.laps;
  setMessage('');
}

// ---- Front-end menus (docs/frontend.md §2.2; camera nodes in brackets) ----

const NODE = { MAIN: 1, SELECT_CAR: 2, CAR_CHOSEN: 3, SELECT_TRACK: 4, OPTIONS: 11, ENTER_NAME: 12, STUNT: 13, SELECT_RACE: 14,
  GO: 16, SUMMARY: 17, RETURN: 18, WORKBENCH: 24 };
const onOff = () => [{ value: true, label: str(63, 'On') }, { value: false, label: str(64, 'Off') }];
const numbers = (list) => list.map((n) => ({ value: n, label: String(n) }));

function mainMenu() {
  return {
    node: NODE.MAIN,
    items: [
      { label: str(14, 'Start Race'), action: () => menu.show(selectRace()) },
      { label: str(18, 'Options'), action: () => menu.show(optionsMenu()) },
      { label: str(50, 'View Credits'), action: () => menu.show(creditsPage(0)) },
    ],
  };
}

// ---- Credits ------------------------------------------------------------
// The original game's credits roll (credits.js), one page per screen.
// Long lists share rows so every page fits the 640×480 menu screen, and
// pages too wide for two columns (the copyright notice) are one column.

function packNames(names, width = 36) {
  // Short lists, and the copyright notice's lines (with blank spacers), as is.
  if (names.length <= 4 || names.includes('')) return names;
  const rows = [];
  for (const name of names) {
    const last = rows.length - 1;
    if (last >= 0 && rows[last].length + 2 + name.length <= width) rows[last] += `, ${name}`;
    else rows.push(name);
  }
  return rows;
}

function creditsPage(index) {
  const page = CREDITS[index];
  const roles = page.roles.map(({ role, names }) => ({ role, rows: packNames(names) }));
  const roleWidth = Math.max(0, ...roles.map((r) => r.role.length));
  const nameWidth = Math.max(0, ...roles.flatMap((r) => r.rows.map((n) => n.length)));
  const stacked = (roleWidth + nameWidth) * 8 + 32 > 600;
  const items = [];
  for (const { role, rows } of roles) {
    if (stacked) {
      for (const row of role ? [role, ...rows] : rows) items.push({ label: '', text: row });
    } else {
      rows.forEach((row, k) => items.push({ label: k === 0 ? role : '', text: row }));
    }
  }
  const last = index === CREDITS.length - 1;
  const turn = (delta) => () => {
    const next = index + delta;
    if (next >= 0 && next < CREDITS.length) menu.show(creditsPage(next));
  };
  if (items.length) items.push({ label: '', text: '' });
  items.push({
    label: last ? 'Done' : 'Next', value: `${index + 1} / ${CREDITS.length}`,
    left: turn(-1), right: turn(1),
    action: last ? () => menu.show(mainMenu()) : turn(1),
  });
  return {
    title: page.title, node: NODE.MAIN, back: () => menu.show(mainMenu()),
    y: Math.max(104, Math.round((480 - items.length * 16) / 2)),
    items,
  };
}

// The same roll as a scrolling page, for the start screen before a disc
// (and so the game's font) is loaded.
function showCreditsSheet() {
  const body = $('creditsBody');
  if (!body.childElementCount) {
    const copyright = (t) => t.replaceAll('@', '©');
    for (const page of CREDITS) {
      const section = document.createElement('section');
      const h = document.createElement('h3');
      h.textContent = page.title;
      section.append(h);
      const dl = document.createElement('dl');
      for (const { role, names } of page.roles) {
        const dt = document.createElement('dt');
        dt.textContent = copyright(role);
        const dd = document.createElement('dd');
        names.forEach((n, k) => {
          if (k) dd.append(document.createElement('br'));
          dd.append(copyright(n));
        });
        dl.append(dt, dd);
      }
      if (page.roles.length) section.append(dl);
      body.append(section);
    }
  }
  $('credits').classList.remove('hidden');
  $('creditsClose').focus();
}
const creditsOpen = () => !$('credits').classList.contains('hidden');
function hideCreditsSheet() {
  $('credits').classList.add('hidden');
  $('creditsButton').focus();
}
$('creditsButton').onclick = showCreditsSheet;
$('creditsClose').onclick = hideCreditsSheet;
$('credits').onclick = (e) => { if (e.target === $('credits')) hideCreditsSheet(); };

function selectRace() {
  const pick = (mode) => () => { settings.mode = mode; menu.show(enterName()); };
  return {
    title: str(1, 'Select Race'), node: NODE.SELECT_RACE, back: () => menu.show(mainMenu()),
    items: [
      { label: str(21, 'Single Race'), action: pick('single') },
      { label: str(24, 'Time Trial'), action: pick('trial') },
      { label: str(26, 'Stunt Arena'), action: pick('stunt') },
    ],
  };
}

// Name wheel: A–Z, space, !, delete, done (§3.5).
const WHEEL = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ', ' ', '!', 'DEL', 'END'];
let wheel = 0;
function enterName() {
  const item = {
    label: str(253, 'Player'),
    get value() { return `${settings.name}${WHEEL[wheel].length > 1 ? ` [${WHEEL[wheel]}]` : `${WHEEL[wheel]}_`}`; },
    left: () => turn(-1), right: () => turn(1),
    select: () => {
      if (wheel === 29) {
        if (!settings.name.trim()) return;
        sfx.play('WAVS/MENUNEXT.WAV', 0.6);
        return menu.show(selectCar());
      }
      if (wheel === 28) settings.name = settings.name.slice(0, -1);
      else settings.name += WHEEL[wheel];
      sfx.play('WAVS/MENUUPDOWN.WAV', 0.6);
      if (settings.name.length >= 14) turn(29 - wheel);
    },
  };
  const turn = (d) => { wheel = (wheel + d + 30) % 30; app.fe_set_wheel(wheel); };
  wheel = settings.name ? 29 : 0;
  app.fe_set_wheel(wheel);
  return { title: str(2, 'Enter Name'), node: NODE.ENTER_NAME, y: 120, back: () => menu.show(selectRace()), items: [item] };
}

function selectCar() {
  const cars = orderedCars();
  if (!cars.some((c) => c.id === settings.car)) settings.car = cars[0].id;
  let choosing = false;
  const step = (d) => {
    if (choosing) return;
    const i = cars.findIndex((c) => c.id === settings.car);
    settings.car = cars[(i + d + cars.length) % cars.length].id;
    app.fe_select_car(settings.car);
  };
  const item = {
    label: str(277, 'Car'),
    get value() { return carName(settings.car); },
    left: () => step(-1), right: () => step(1),
    select: async () => {
      if (choosing) return;
      choosing = true;
      sfx.play('WAVS/MENUNEXT.WAV', 0.6);
      // The box flies to the floor and the car drops out (§3.4).
      app.fe_choose_car(true);
      app.fe_goto(NODE.CAR_CHOSEN, 0.85);
      const t0 = performance.now();
      while (!app.fe_car_shown() && performance.now() - t0 < 5000) await new Promise((r) => setTimeout(r, 100));
      await new Promise((r) => setTimeout(r, 1300));
      choosing = false;
      menu.show(settings.mode === 'stunt' ? summary() : selectTrack());
    },
  };
  return {
    title: str(3, 'Select Car'), node: NODE.SELECT_CAR, back: () => { app.fe_select_car(null); menu.show(enterName()); },
    onShow: () => { app.fe_choose_car(false); app.fe_select_car(settings.car); },
    // Only while the box is out on the shelf: once a car is chosen it's on
    // its way to the floor and the menu moves on.
    get corner() {
      if (choosing) return null;
      return {
        label: 'Workbench',
        action: async () => {
          choosing = true;
          await app.fe_select_car(settings.car);
          app.fe_workbench(true);
          menu.show(workbench());
        },
      };
    },
    items: [item],
  };
}

// ---- Workbench ------------------------------------------------------------
// New to this version: the chosen car stands on the shop counter, where it
// can be renamed and tuned. Changes are multipliers of the stock values
// (src/tuning.rs), saved per car in localStorage.

const TUNING = [
  ['mass', 'Weight'], ['stiffness', 'Spring Stiffness'], ['damping', 'Spring Damping'], ['travel', 'Suspension Travel'],
  ['grip', 'Tyre Grip'], ['power', 'Engine Power'], ['top_speed', 'Top Speed'], ['steering', 'Steering Speed'],
  ['downforce', 'Downforce'],
];
const PERCENTS = Array.from({ length: 16 }, (_, i) => (50 + i * 10) / 100).map((v) => ({ value: v, label: `${Math.round(v * 100)}%` }));
const tunings = loadTunings();

function loadTunings() {
  try {
    const t = JSON.parse(localStorageGet('carTuning') ?? '{}');
    return t && typeof t === 'object' ? t : {};
  } catch { return {}; }
}

// Saves one car's tuning and rebuilds the car with it.
async function saveTuning(id) {
  const t = tunings[id];
  if (t && !Object.keys(t).length) delete tunings[id];
  localStorageSet('carTuning', JSON.stringify(tunings));
  applied.car = null; // reload the player's car before the next race
  await app.set_car_tuning(id, tunings[id] ?? null);
}

function workbench(selected = 0) {
  const id = settings.car;
  const t = () => (tunings[id] ??= {});
  const items = [
    { label: 'Name', get value() { return carName(id); }, action: () => menu.show(renameCar()) },
    ...TUNING.map(([key, label]) => ({
      label, options: PERCENTS,
      value: tunings[id]?.[key] ?? 1,
      onChange: (v) => {
        if (v === 1) delete t()[key];
        else t()[key] = v;
        saveTuning(id);
      },
    })),
    { label: '', text: '' },
    {
      label: 'Reset to Stock',
      get disabled() { return !tunings[id]; },
      action: () => {
        delete tunings[id];
        saveTuning(id);
        menu.show(workbench(menu.index));
      },
    },
    { label: 'Done', action: () => leave() },
  ];
  const leave = () => { app.fe_workbench(false); menu.show(selectCar()); };
  return {
    title: 'Workbench', node: NODE.WORKBENCH, x: 48, y: 128, back: leave, items,
    onShow: () => { if (selected) menu.index = selected; },
  };
}

// Renaming: type, or turn through the letters with left/right and pick one
// with Enter/A (like the name wheel).
const NAME_LETTERS = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789', ' ', '-', '!', 'DEL', 'END'];
const CAR_NAME_MAX = 16;
function renameCar() {
  const id = settings.car;
  let draft = carName(id);
  let letter = NAME_LETTERS.length - 1;
  const done = () => {
    const name = draft.trim();
    if (name && name !== nameOf(catalog.cars, id)) (tunings[id] ??= {}).name = name;
    else if (tunings[id]) delete tunings[id].name;
    saveTuning(id);
    menu.show(workbench());
  };
  const add = (c) => { if (draft.length < CAR_NAME_MAX) draft += c; };
  const item = {
    get label() { return `${draft}${NAME_LETTERS[letter].length > 1 ? ` [${NAME_LETTERS[letter]}]` : `${NAME_LETTERS[letter]}_`}`; },
    left: () => { letter = (letter + NAME_LETTERS.length - 1) % NAME_LETTERS.length; },
    right: () => { letter = (letter + 1) % NAME_LETTERS.length; },
    select: () => {
      const c = NAME_LETTERS[letter];
      if (c === 'END') return done();
      if (c === 'DEL') draft = draft.slice(0, -1);
      else add(c);
      sfx.play('WAVS/MENUUPDOWN.WAV', 0.6);
    },
  };
  return {
    title: 'Rename Car', node: NODE.WORKBENCH, x: 48, y: 160,
    back: () => menu.show(workbench()),
    items: [
      item,
      { label: 'Type, or turn letters', text: '' },
      { label: 'Original Name', action: () => { draft = nameOf(catalog.cars, id); } },
      { label: 'Done', action: done },
    ],
    // Typing goes to the name while its row is highlighted.
    onKey: (e) => {
      if (menu.index !== 0) return false;
      if (e.key === 'Enter') { done(); return true; }
      if (e.key === 'Backspace') { draft = draft.slice(0, -1); return true; }
      if (e.key.length === 1 && /[A-Za-z0-9 !\-.']/.test(e.key) && !e.ctrlKey && !e.metaKey) { add(e.key); return true; }
      return false;
    },
  };
}

function selectTrack() {
  const tracks = raceTracks();
  if (!tracks.some((t) => t.id === settings.level)) settings.level = tracks[0].id;
  const step = (d) => {
    const i = tracks.findIndex((t) => t.id === settings.level);
    settings.level = tracks[(i + d + tracks.length) % tracks.length].id;
    app.fe_set_track(settings.level);
  };
  return {
    title: str(4, 'Select Track'), node: NODE.SELECT_TRACK, y: 380,
    back: () => menu.show(selectCar()),
    onShow: () => app.fe_set_track(settings.level),
    items: [{ label: str(460, 'Track'), get value() { return nameOf(catalog.levels, settings.level); },
      left: () => step(-1), right: () => step(1), action: () => menu.show(summary()) }],
  };
}

function summary() {
  const want = effective();
  const modes = { single: str(21, 'Single Race'), trial: str(24, 'Time Trial'), stunt: str(26, 'Stunt Arena') };
  const info = [
    { label: 'Mode', text: modes[settings.mode] },
    { label: str(460, 'Track'), text: nameOf(catalog.levels, want.level) },
    { label: str(277, 'Car'), text: carName(want.car) },
  ];
  if (settings.mode !== 'stunt') info.push({ label: str(35, 'Laps'), text: String(want.laps) });
  if (settings.mode === 'single') info.push({ label: 'Opponents', text: String(want.opponents) },
    { label: str(54, 'Pick ups'), text: want.pickups ? str(63, 'On') : str(64, 'Off') });
  return {
    title: str(6, 'Summary'), node: NODE.SUMMARY,
    back: () => menu.show(settings.mode === 'stunt' ? selectCar() : selectTrack()),
    items: [...info, { label: str(14, 'Start Race'), action: go }],
  };
}

// Race start: quick move to the "go" shot, then into the race (§5).
async function go() {
  app.fe_goto(NODE.GO, 0.25);
  menu.hide();
  await new Promise((r) => setTimeout(r, 400));
  await startRace();
}

function optionsMenu() {
  return {
    title: str(18, 'Options'), node: NODE.OPTIONS, back: () => menu.show(mainMenu()),
    items: [
      { label: str(45, 'Game Settings'), action: () => menu.show(gameSettings()) },
      { label: str(47, 'Audio Settings'), action: () => menu.show(audioSettings()) },
    ],
  };
}

function gameSettings() {
  return {
    title: str(45, 'Game Settings'), node: NODE.OPTIONS, back: () => menu.show(optionsMenu()),
    items: [
      { label: str(51, 'Number of Cars'), options: numbers([1, 2, 3, 4, 5, 6, 7, 8]), value: settings.opponents + 1,
        onChange: (v) => { settings.opponents = v - 1; localStorageSet('opponents', String(v - 1)); } },
      { label: str(55, 'Number of Laps'), options: numbers([1, 2, 3, 4, 5, 10]), value: settings.laps,
        onChange: (v) => { settings.laps = v; localStorageSet('laps', String(v)); } },
      { label: str(54, 'Pick ups'), options: onOff(), value: settings.pickups,
        onChange: (v) => { settings.pickups = v; localStorageSet('pickups', String(v)); } },
    ],
  };
}

function audioSettings() {
  return {
    title: str(47, 'Audio Settings'), node: NODE.OPTIONS, back: () => menu.show(optionsMenu()),
    items: [
      { label: str(98, 'In-Game Music'), options: onOff(), value: music.enabled, onChange: (v) => { if (v !== music.enabled) toggleMusic(); } },
      { label: 'Sound Effects', options: onOff(), value: sfx.enabled, onChange: (v) => { if (v !== sfx.enabled) toggleSound(); } },
    ],
  };
}

// Enter the toy shop (the front end) at `node`, e.g. returning from a race.
function enterShop(fromNode) {
  document.body.classList.add('in-shop');
  app.set_paused(true);
  app.show_frontend(true);
  app.fe_choose_car(false);
  app.fe_select_car(null);
  if (fromNode !== undefined) app.fe_goto(fromNode, 0);
  music.playRange(2, 2); // the front end's REDBOOK 0 0
  menu.show(mainMenu());
}

async function startRace() {
  menu.hide();
  document.body.classList.remove('in-shop');
  app.show_frontend(false);
  try {
    await applySettings();
    const range = app.level_music();
    if (range) music.playRange(range[0] + REDBOOK_TRACK_OFFSET, range[1] + REDBOOK_TRACK_OFFSET);
  } catch (e) {
    setMessage(`Could not start: ${e}`);
  }
  app.set_paused(false);
  canvas.focus();
}

// ---- In-race menus ------------------------------------------------------

function pause() {
  if (menu.open || applied.level === null || app.frontend_shown()) return;
  app.set_paused(true);
  menu.show({
    title: 'Paused', back: resume,
    items: [
      { label: str(132, 'Resume'), action: resume },
      { label: str(136, 'Restart Race'), action: restart },
      { label: str(19, 'Quit'), action: () => enterShop(NODE.RETURN) },
    ],
  });
}

function resume() {
  menu.hide();
  app.set_paused(false);
}

function restart() {
  menu.hide();
  app.reset_car();
  app.set_paused(false);
}

function showResults(r) {
  const rows = r.standings.map((s, i) => ({
    label: `${ordinal(i + 1)} ${s.player ? `${settings.name || s.name}` : s.name}`,
    text: s.time === null ? '--:--.--' : clock(s.time),
  }));
  menu.show({
    title: `${ordinal(r.position)} place`,
    items: [...rows, { label: str(136, 'Restart Race'), action: restart }, { label: str(19, 'Quit'), action: () => enterShop(NODE.RETURN) }],
  });
}

// ---- Disc opening -------------------------------------------------------

// Serves byte ranges lazily from a File, so the ~160 MB data track is never
// read into memory in full.
const readerFor = (file) => (offset, length) =>
  file.slice(offset, offset + length).arrayBuffer().then((b) => {
    loader.read(b.byteLength);
    return new Uint8Array(b);
  });

// ---- Loading bar --------------------------------------------------------
// Opening a disc and building the shop takes a few seconds. All the engine's
// reads go through readerFor, so each step's progress is the bytes it has
// read so far against what that step read last time (remembered in
// localStorage; the defaults are measured from the USA disc).

const LOAD_STEPS = [
  { key: 'disc', label: 'Reading the disc', bytes: 650_000 },
  { key: 'menus', label: 'Loading the menus', bytes: 300_000 },
  // The shop's last stretch is building it from what was read, which reads
  // nothing, so its reads only take the bar to 85%.
  { key: 'shop', label: 'Building the toy shop', bytes: 4_360_000, reads: 0.85 },
];
const loader = {
  active: false,
  step: 0,
  bytes: 0, // read during the current step
  used: {}, // bytes each finished step read, saved for next time
  queued: false,
  expected: { ...Object.fromEntries(LOAD_STEPS.map((s) => [s.key, s.bytes])), ...loadJson('loadBytes') },

  begin(fromCache) {
    Object.assign(this, { active: true, step: 0, bytes: 0, used: {} });
    this.fromCache = fromCache;
    setMessage(''); // the bar says what's happening
    $('intro').classList.add('hidden');
    $('loading').classList.remove('hidden');
    $('drop').classList.remove('hidden');
    $('pick').disabled = true;
    this.draw();
  },
  next(key) {
    this.used[LOAD_STEPS[this.step].key] = this.bytes;
    this.step = LOAD_STEPS.findIndex((s) => s.key === key);
    this.bytes = 0;
    this.draw();
  },
  read(n) {
    if (!this.active) return;
    this.bytes += n;
    if (!this.queued) {
      this.queued = true;
      requestAnimationFrame(() => { this.queued = false; this.draw(); });
    }
  },
  // Overall fraction: finished steps plus the current one, which is held
  // short of full until it actually finishes.
  fraction() {
    const sizes = LOAD_STEPS.map((s) => Math.max(1, this.expected[s.key] || 1));
    const total = sizes.reduce((a, b) => a + b, 0);
    const before = sizes.slice(0, this.step).reduce((a, b) => a + b, 0);
    const size = sizes[this.step];
    return (before + Math.min(this.bytes / size, 1) * size * (LOAD_STEPS[this.step].reads ?? 0.97)) / total;
  },
  draw() {
    if (!this.active) return;
    const pct = Math.round(this.fraction() * 100);
    const step = LOAD_STEPS[this.step];
    const label = this.step === 0 && this.fromCache ? 'Opening your saved disc' : step.label;
    $('loadingFill').style.width = `${pct}%`;
    $('loading').setAttribute('aria-valuenow', pct);
    $('loadingLabel').textContent = `${label}… ${pct}%`;
  },
  finish() {
    this.used[LOAD_STEPS[this.step].key] = this.bytes;
    this.active = false;
    saveJson('loadBytes', this.used);
    $('loadingFill').style.width = '100%';
    $('drop').classList.add('hidden');
    this.reset();
  },
  fail() {
    this.active = false;
    $('intro').classList.remove('hidden');
    this.reset();
  },
  reset() {
    $('loading').classList.add('hidden');
    $('pick').disabled = false;
  },
};

// Of the chosen files, the data track is the one the engine accepts as an
// ISO9660 filesystem; .cue sheets and audio tracks are skipped.
async function openFiles(files) {
  const candidates = [...files]
    .filter((f) => !/\.cue$/i.test(f.name))
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
  if (!candidates.length) throw new Error('Select the .bin track files (or an .iso), not just the .cue.');
  let lastError;
  for (const file of candidates) {
    try {
      const result = await app.open_disc(readerFor(file));
      music.setTracks(await findAudioTracks(files));
      return result;
    } catch (e) {
      lastError = e;
    }
  }
  throw new Error(`No Re-Volt data track found (${lastError})`);
}

async function start(files, fromCache = false) {
  if (loader.active) return;
  loader.begin(fromCache);
  try {
    catalog = await openFiles(files);
    if (!catalog.levels.some((l) => l.id === settings.level)) settings.level = catalog.levels[0].id;
    if (!catalog.cars.some((c) => c.id === settings.car)) settings.car = catalog.cars[0].id;
    // Into the toy shop: strings and font from the disc, then the main
    // menu while the last-used race loads in the background.
    loader.next('menus');
    for (const [id, t] of Object.entries(tunings)) await app.set_car_tuning(id, t);
    S = Array.from(await app.strings());
    await menu.loadFont(await app.read_image('GFX/FONT.BMP'));
    loader.next('shop');
    await app.load_frontend();
    loader.finish();
    document.querySelectorAll('.needs-disc').forEach((el) => el.classList.remove('hidden'));
    enterShop();
  } catch (e) {
    loader.fail();
    setMessage(String(e.message ?? e));
    return;
  }
  try {
    await applySettings();
    app.set_paused(true);
  } catch (e) {
    setMessage(String(e.message ?? e));
  }
  // Saved for next visit once everything's loaded: storing a fresh disc
  // copies all its tracks, which slows the disc reads above if it overlaps.
  if (!fromCache) remember([...files]);
}

$('pick').onclick = () => $('file').click();
$('file').onchange = (e) => e.target.files.length && start(e.target.files);
$('menuButton').onclick = (e) => { pause(); e.target.blur(); };
$('camera').onclick = (e) => { app.toggle_camera(); e.target.blur(); };
$('flip').onclick = (e) => { app.flip_car(); e.target.blur(); };
function toggleMusic() {
  music.setEnabled(!music.enabled);
  localStorageSet('music', music.enabled ? 'on' : 'off');
  $('music').textContent = music.enabled ? 'Music: on' : 'Music: off';
}
$('music').textContent = music.enabled ? 'Music: on' : 'Music: off';
$('music').onclick = (e) => { toggleMusic(); e.target.blur(); };
function toggleSound() {
  sfx.setEnabled(!sfx.enabled);
  localStorageSet('sound', sfx.enabled ? 'on' : 'off');
  $('sound').textContent = sfx.enabled ? 'Sound: on' : 'Sound: off';
}
$('sound').textContent = sfx.enabled ? 'Sound: on' : 'Sound: off';
$('sound').onclick = (e) => { toggleSound(); e.target.blur(); };

// Browsers only allow audio after a user gesture.
for (const type of ['pointerdown', 'keydown']) addEventListener(type, () => music.unlock());

addEventListener('dragover', (e) => e.preventDefault());
addEventListener('drop', (e) => {
  e.preventDefault();
  if (e.dataTransfer.files.length) start(e.dataTransfer.files);
});

// ---- Remembering the disc between visits --------------------------------
// The chosen disc files are stored in IndexedDB (the browser keeps its own
// copy), so returning visitors skip the file picker and keep their music.

const DB = 'revolt-web';
function idb(mode, fn) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open(DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore('disc');
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const tx = open.result.transaction('disc', mode);
      const req = fn(tx.objectStore('disc'));
      tx.oncomplete = () => resolve(req.result);
      tx.onerror = () => reject(tx.error);
    };
  });
}
async function remember(files) {
  try { await idb('readwrite', (s) => s.put(files, 'track')); } catch { /* storage may be unavailable */ }
}
async function recall() {
  try {
    const saved = await idb('readonly', (s) => s.get('track'));
    return saved instanceof Blob ? [saved] : saved; // older versions stored one File
  } catch { return undefined; }
}
function localStorageGet(k) { try { return localStorage.getItem(k); } catch { return null; } }
function localStorageSet(k, v) { try { localStorage.setItem(k, v); } catch { /* ignore */ } }
function loadJson(k) { try { return JSON.parse(localStorageGet(k)) ?? {}; } catch { return {}; } }
function saveJson(k, v) { localStorageSet(k, JSON.stringify(v)); }

const saved = await recall();
if (saved?.length) start(saved, true);

// ---- Input --------------------------------------------------------------

const VIEW_KEYS = new Set(['KeyW', 'KeyA', 'KeyS', 'KeyD', 'KeyQ', 'KeyE', 'KeyR', 'KeyF', 'KeyC', 'Space', 'ShiftLeft', 'ShiftRight',
  'ControlLeft', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']);
addEventListener('keydown', (e) => {
  if (creditsOpen()) {
    if (e.code === 'Escape') hideCreditsSheet();
    return;
  }
  if (menu.key(e)) return;
  if (e.code === 'Escape' && !e.repeat) return pause();
  if (VIEW_KEYS.has(e.code)) e.preventDefault();
  if (e.code === 'KeyM' && !e.repeat) toggleMusic();
  if (e.code === 'KeyN' && !e.repeat) toggleSound();
  app.key(e.code, true);
});
addEventListener('keyup', (e) => app.key(e.code, false));
addEventListener('blur', () => app.release_keys());
canvas.addEventListener('click', () => { if (!menu.open) canvas.requestPointerLock?.(); });
addEventListener('mousemove', (e) => {
  if (document.pointerLockElement === canvas) app.mouse_move(e.movementX, e.movementY);
});

// ---- Race HUD -------------------------------------------------------------

const hud = $('hud');
const banner = $('banner');
const ordinal = (n) => n + (['th', 'st', 'nd', 'rd'][(n % 100 - 20) % 10] || ['th', 'st', 'nd', 'rd'][n % 100] || 'th');
const clock = (s) => `${Math.floor(s / 60)}:${(s % 60).toFixed(2).padStart(5, '0')}`;
let lastCountdown = 0;
let goUntil = 0;
let finishedAt = null;
let resultsShown = false;

function updateHud(t) {
  const r = app.frontend_shown() ? null : app.race_state();
  if (!r) {
    hud.classList.add('hidden');
    banner.textContent = '';
    return;
  }
  // A new countdown started (load, restart): play the countdown sound.
  if (r.countdown > lastCountdown + 0.5) {
    sfx.play('WAVS/COUNTDOWN.WAV', 0.8);
    finishedAt = null;
    resultsShown = false;
  }
  if (lastCountdown > 0 && r.countdown === 0) goUntil = t + 1000;
  lastCountdown = r.countdown;

  hud.classList.remove('hidden');
  const place = r.racers > 1 && r.lap !== null ? `<div class="big">${ordinal(r.position)} / ${r.racers}</div>` : '';
  const weaponLine = r.bomb != null
    ? `<div class="weapon bomb">BOMB ${r.bomb > 0 ? r.bomb.toFixed(1) : '!!'}</div>`
    : r.weapon ? `<div class="weapon${r.rolling ? ' rolling' : ''}">${r.weapon}${r.uses > 1 ? ` ×${r.uses}` : ''}</div>` : '';
  const zapLine = r.zapped ? '<div class="weapon bomb">ZAPPED</div>' : '';
  hud.innerHTML = r.lap === null
    ? `<div class="big">${clock(r.time)}</div><div>Free roam</div>`
    : `${place}<div class="big">LAP ${r.lap}/${r.laps}</div><div>${clock(r.time)}</div>` +
      `<div>Lap ${clock(r.lapTime)}</div>${r.best === null ? '' : `<div>Best ${clock(r.best)}</div>`}`;
  hud.innerHTML += weaponLine + zapLine;

  // Results a moment after the player finishes; refresh while AI cars finish.
  if (r.finished !== null) {
    finishedAt ??= t;
    if (!resultsShown && t - finishedAt > 2500) {
      resultsShown = true;
      showResults(r);
    } else if (resultsShown && menu.open && menu.screen.title.endsWith('place')) {
      const index = menu.index;
      showResults(r);
      menu.index = index;
      menu.render();
    }
  }

  let text = '';
  let cls = '';
  if (menu.open) text = '';
  else if (r.countdown > 0) text = String(Math.ceil(r.countdown));
  else if (t < goUntil) text = 'GO!';
  else if (r.finished !== null) { text = `${ordinal(r.position)} PLACE`; cls = 'done'; }
  else if (r.wrongWay) { text = 'WRONG WAY'; cls = 'warn'; }
  banner.textContent = text;
  banner.className = cls;
}

// ---- Main loop ----------------------------------------------------------

let lastStatus = 0;
function frame(t) {
  const pressed = menu.poll(t);
  if (creditsOpen()) {
    if (pressed.has('b') || pressed.has('start')) hideCreditsSheet();
  } else if (pressed.has('start') && !menu.open) pause();
  app.frame(t);
  sfx.update(menu.open ? null : app.sound_state());
  for (const s of app.take_sounds()) sfx.play(s.path, s.volume);
  updateHud(t);
  if (t - lastStatus > 250) {
    lastStatus = t;
    status.textContent = message || app.status();
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
