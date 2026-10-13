// Menus drawn like the original front end (docs/frontend.md §2, §4): the
// game's bitmap font (GFX/FONT.BMP, 8×16 cells, 32 per row) on a 640×480
// virtual screen, titles in 16×24 glyphs at (140, 44), items in 8×12 glyphs
// on 16 px rows, panels sliding in from the left at 1500 px/s and out
// downwards. Gamepad, keyboard and mouse.
//
// A screen is { title, items, back?, node?, y?, onShow? }. Items are either
//   { label, action }                                       — a button,
//   { label, options: [{value, label}], value, onChange }   — a choice,
//   { label, text }                                         — read-only info;
// any item may set `left`/`right`/`select` handlers, `disabled`, and `node`
// (front-end camera node while it's highlighted).

const VW = 640;
const VH = 480;
const SLIDE = 1500; // px/s
const REPEAT_DELAY = 0.35;
const REPEAT_RATE = 0.09;
const COLORS = { normal: '#ffffff', selected: '#ff7722', disabled: '#444444', disabledSelected: '#803010', title: '#ffffff', hint: '#9aa3b5' };

export class Menu {
  constructor(canvas, playSound = () => {}) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.playSound = playSound;
    this.screen = null;
    this.index = 0;
    this.held = new Map();
    this.lastPoll = 0;
    this.font = null; // { [color]: canvas }
    this.panel = { x: 0, y: 0, tx: 0, ty: 0, state: 'idle' };
    this.next = null;
    this.onNode = () => {};
    canvas.addEventListener('click', (e) => this.click(e));
    canvas.addEventListener('mousemove', (e) => this.hover(e));
  }

  get open() {
    return this.screen !== null;
  }

  // Loads the game font and makes one tinted copy per colour.
  async loadFont(image) {
    const sheet = document.createElement('canvas');
    sheet.width = image.width;
    sheet.height = image.height;
    sheet.getContext('2d').putImageData(new ImageData(image.rgba, image.width, image.height), 0, 0);
    this.font = {};
    for (const color of new Set(Object.values(COLORS))) {
      const c = document.createElement('canvas');
      c.width = sheet.width;
      c.height = sheet.height;
      const g = c.getContext('2d');
      g.drawImage(sheet, 0, 0);
      g.globalCompositeOperation = 'multiply';
      g.fillStyle = color;
      g.fillRect(0, 0, c.width, c.height);
      g.globalCompositeOperation = 'destination-in';
      g.drawImage(sheet, 0, 0);
      this.font[color] = c;
    }
  }

  show(screen) {
    if (document.pointerLockElement) document.exitPointerLock();
    this.canvas.classList.remove('hidden');
    if (this.screen && this.panel.state !== 'closing') {
      // Slide the current panel out, then the new one in.
      this.next = screen;
      this.panel.state = 'closing';
      this.panel.ty = 580;
      return;
    }
    this.enter(screen);
  }

  enter(screen) {
    this.screen = screen;
    this.index = Math.max(0, screen.items.findIndex((i) => !i.disabled && i.text === undefined));
    const { w, h } = this.size();
    this.panel = { x: -w - 64, y: screen.y ?? Math.round((VH - h) / 2), tx: Math.round((VW - w) / 2), ty: screen.y ?? Math.round((VH - h) / 2), state: 'opening' };
    this.next = null;
    this.reportNode();
    screen.onShow?.();
  }

  hide() {
    this.screen = null;
    this.next = null;
    this.canvas.classList.add('hidden');
  }

  // Re-run after an item's label/value changes.
  render() {}

  size() {
    const items = this.screen?.items ?? [];
    const label = Math.max(0, ...items.map((i) => i.label.length));
    const value = Math.max(0, ...items.map((i) => this.valueText(i).length));
    const w = label * 8 + value * 8 + (value ? 32 : 0);
    return { w, h: items.length * 16, label };
  }

  valueText(item) {
    if (item.text !== undefined) return String(item.text);
    if (item.options) return item.options.find((o) => o.value === item.value)?.label ?? String(item.value);
    if (item.value !== undefined) return String(item.value);
    return '';
  }

  reportNode() {
    const item = this.screen?.items[this.index];
    const node = item?.node ?? this.screen?.node;
    if (node !== undefined) this.onNode(node);
  }

  move(delta) {
    const items = this.screen.items;
    for (let k = 1; k <= items.length; k++) {
      const i = (this.index + delta * k + items.length * 4) % items.length;
      if (!items[i].disabled && items[i].text === undefined) {
        this.index = i;
        break;
      }
    }
    this.playSound('WAVS/MENUUPDOWN.WAV');
    this.reportNode();
  }

  change(delta) {
    const item = this.screen.items[this.index];
    if (!item || item.disabled) return;
    if (delta < 0 && item.left) { item.left(); this.playSound('WAVS/MENULEFTRIGHT.WAV'); return; }
    if (delta > 0 && item.right) { item.right(); this.playSound('WAVS/MENULEFTRIGHT.WAV'); return; }
    if (!item.options) return;
    const i = item.options.findIndex((o) => o.value === item.value);
    const next = item.options[(i + delta + item.options.length) % item.options.length];
    item.value = next.value;
    this.playSound('WAVS/MENULEFTRIGHT.WAV');
    item.onChange?.(next.value);
  }

  activate() {
    const item = this.screen.items[this.index];
    if (!item || item.disabled) return;
    if (item.select) { item.select(); return; }
    if (item.options) return this.change(1);
    if (!item.action) return;
    this.playSound('WAVS/MENUNEXT.WAV');
    item.action();
  }

  back() {
    if (this.screen?.back) {
      this.playSound('WAVS/MENUPREV.WAV');
      this.screen.back();
    }
  }

  key(e) {
    if (!this.open) return false;
    const map = { ArrowUp: () => this.move(-1), ArrowDown: () => this.move(1), ArrowLeft: () => this.change(-1),
      ArrowRight: () => this.change(1), Enter: () => this.activate(), Space: () => this.activate(), Escape: () => this.back(),
      Backspace: () => this.back() };
    const fn = map[e.code];
    if (!fn) return false;
    e.preventDefault();
    if (this.panel.state === 'idle' || this.panel.state === 'opening') fn();
    return true;
  }

  // Virtual-screen transform for the current canvas size.
  layout() {
    const dpr = Math.min(2, devicePixelRatio || 1);
    const w = Math.round(this.canvas.clientWidth * dpr);
    const h = Math.round(this.canvas.clientHeight * dpr);
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    const scale = Math.min(w / VW, h / VH);
    return { scale, ox: (w - VW * scale) / 2, oy: (h - VH * scale) / 2, dpr };
  }

  toVirtual(e) {
    const { scale, ox, oy, dpr } = this.layout();
    const r = this.canvas.getBoundingClientRect();
    return { x: ((e.clientX - r.left) * dpr - ox) / scale, y: ((e.clientY - r.top) * dpr - oy) / scale };
  }

  itemAt(e) {
    if (!this.screen || this.panel.state !== 'idle') return -1;
    const { x, y } = this.toVirtual(e);
    const { w } = this.size();
    const row = Math.floor((y - this.panel.y + 2) / 16);
    if (x < this.panel.x - 20 || x > this.panel.x + w + 20 || row < 0 || row >= this.screen.items.length) return -1;
    const item = this.screen.items[row];
    return item.disabled || item.text !== undefined ? -1 : row;
  }

  hover(e) {
    const i = this.itemAt(e);
    if (i >= 0 && i !== this.index) {
      this.index = i;
      this.reportNode();
    }
  }

  click(e) {
    const i = this.itemAt(e);
    if (i < 0) return;
    this.index = i;
    const item = this.screen.items[i];
    if (item.options || item.left || item.right) {
      // Click the left part of the row to go back a value, else forward.
      const { x } = this.toVirtual(e);
      const { w } = this.size();
      if (item.select && x > this.panel.x + w * 0.66) return this.activate();
      this.change(x < this.panel.x + w / 2 ? -1 : 1);
    } else {
      this.activate();
    }
  }

  text(g, str, x, y, gw, gh, color) {
    const sheet = this.font?.[color];
    for (let k = 0; k < str.length; k++) {
      const c = str.charCodeAt(k);
      if (c >= 33 && sheet) {
        const i = c - 33;
        g.drawImage(sheet, (i % 32) * 8, Math.floor(i / 32) * 16 + 1, 8, 16, x + k * gw, y, gw, gh);
      } else if (c >= 33) {
        g.fillStyle = color;
        g.font = `${gh}px monospace`;
        g.fillText(str[k], x + k * gw, y + gh * 0.85);
      }
    }
  }

  // Draws the menu; call once per frame.
  draw(dt) {
    if (!this.screen) return;
    const p = this.panel;
    // x first, then y, at 1500 px/s (menu_update_slide).
    const step = SLIDE * dt;
    if (p.x !== p.tx) p.x += Math.sign(p.tx - p.x) * Math.min(step, Math.abs(p.tx - p.x));
    else if (p.y !== p.ty) p.y += Math.sign(p.ty - p.y) * Math.min(step, Math.abs(p.ty - p.y));
    if (p.state === 'opening' && p.x === p.tx && p.y === p.ty) p.state = 'idle';
    if (p.state === 'closing' && p.y === p.ty) {
      if (this.next) this.enter(this.next);
      else this.hide();
      if (!this.screen) return;
    }

    const { scale, ox, oy } = this.layout();
    const g = this.ctx;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.clearRect(0, 0, this.canvas.width, this.canvas.height);
    g.setTransform(scale, 0, 0, scale, ox, oy);
    g.imageSmoothingEnabled = false;

    const s = this.screen;
    if (s.title) {
      g.fillStyle = 'rgba(8, 10, 24, 0.72)';
      g.fillRect(108, 40, s.title.length * 16 + 64, 32);
      this.text(g, s.title, 140, 44, 16, 24, COLORS.title);
    }
    const { w, label } = this.size();
    g.fillStyle = 'rgba(8, 10, 24, 0.62)';
    g.fillRect(p.x - 20, p.y - 24, w + 40, s.items.length * 16 + 44);
    s.items.forEach((item, i) => {
      const sel = i === this.index && item.text === undefined;
      const color = item.disabled ? (sel ? COLORS.disabledSelected : COLORS.disabled) : sel ? COLORS.selected : item.text !== undefined ? COLORS.hint : COLORS.normal;
      const y = p.y + 16 * i;
      this.text(g, item.label, p.x, y, 8, 12, color);
      const value = this.valueText(item);
      if (value) this.text(g, value, p.x + label * 8 + 32, y, 8, 12, item.text !== undefined ? COLORS.normal : color);
    });
    this.text(g, 'v1.00', (78 - 5) * 8, 448, 8, 12, COLORS.normal);
  }

  // Gamepad polling with key-repeat; returns newly pressed buttons.
  poll(t) {
    const dt = this.lastPoll ? Math.min(0.1, (t - this.lastPoll) / 1000) : 0;
    this.lastPoll = t;
    const pad = [...(navigator.getGamepads?.() ?? [])].find((q) => q?.connected);
    const pressed = new Set();
    if (pad) {
      const button = (i) => (pad.buttons[i]?.value ?? 0) > 0.5;
      const axis = (i) => pad.axes[i] ?? 0;
      const inputs = {
        up: button(12) || axis(1) < -0.6, down: button(13) || axis(1) > 0.6,
        left: button(14) || axis(0) < -0.6, right: button(15) || axis(0) > 0.6,
        a: button(0), b: button(1), start: button(9),
      };
      for (const [name, down] of Object.entries(inputs)) {
        const prev = this.held.get(name);
        if (!down) { this.held.delete(name); continue; }
        if (prev === undefined) { pressed.add(name); this.held.set(name, 0); continue; }
        const next = prev + dt;
        if (['up', 'down', 'left', 'right'].includes(name) && next > REPEAT_DELAY &&
            Math.floor((next - REPEAT_DELAY) / REPEAT_RATE) > Math.floor((prev - REPEAT_DELAY) / REPEAT_RATE)) {
          pressed.add(name);
        }
        this.held.set(name, next);
      }
    } else {
      this.held.clear();
    }
    if (this.open && (this.panel.state === 'idle' || this.panel.state === 'opening')) {
      if (pressed.has('up')) this.move(-1);
      if (pressed.has('down')) this.move(1);
      if (pressed.has('left')) this.change(-1);
      if (pressed.has('right')) this.change(1);
      if (pressed.has('a')) this.activate();
      if (pressed.has('b')) this.back();
    }
    this.draw(dt);
    return pressed;
  }
}
