// WebXR: race in a VR headset. The engine draws each eye from the chase
// camera (App.frame_xr); this module runs the session, reads the VR
// controllers and paints the HUD panel that floats ahead of the player.
//
// Controllers (xr-standard mapping): right trigger accelerates, left
// trigger brakes and reverses, either thumbstick steers, A fires, B flips
// the car, X changes the camera, Y pauses (which leaves VR). A regular
// gamepad keeps working too.

const HUD_W = 1024;
const HUD_H = 256;

export class XrMode {
  constructor(app, canvas, hooks) {
    this.app = app;
    this.canvas = canvas;
    this.hooks = hooks; // { frame(t), hud() -> {left, right, banner, warn}, pause() }
    this.session = null;
    this.space = null;
    this.prev = new Map(); // button states per hand, for presses
    this.hud = document.createElement('canvas');
    this.hud.width = HUD_W;
    this.hud.height = HUD_H;
    this.lastHud = '';
  }

  get active() {
    return this.session !== null;
  }

  static async supported() {
    try {
      return Boolean(await navigator.xr?.isSessionSupported('immersive-vr'));
    } catch {
      return false;
    }
  }

  async start() {
    if (this.session) return;
    const session = await navigator.xr.requestSession('immersive-vr');
    const gl = this.canvas.getContext('webgl2');
    await gl.makeXRCompatible?.();
    session.updateRenderState({ baseLayer: new XRWebGLLayer(session, gl), depthNear: 0.05, depthFar: 2000 });
    this.space = await session.requestReferenceSpace('local');
    this.session = session;
    this.prev.clear();
    this.lastHud = '';
    session.addEventListener('end', () => { this.session = null; });
    session.requestAnimationFrame((t, f) => this.frame(t, f));
  }

  end() {
    this.session?.end();
  }

  frame(t, xrFrame) {
    const session = this.session;
    if (!session) return;
    session.requestAnimationFrame((t2, f) => this.frame(t2, f));
    const pose = xrFrame.getViewerPose(this.space);
    if (!pose) return;
    const layer = session.renderState.baseLayer;
    const views = pose.views.map((v) => {
      const vp = layer.getViewport(v);
      return { x: vp.x, y: vp.y, w: vp.width, h: vp.height, proj: v.projectionMatrix, view: v.transform.inverse.matrix };
    });
    this.readControllers(session);
    this.paintHud();
    this.app.frame_xr(t, layer.framebuffer, views);
    this.hooks.frame(t);
  }

  readControllers(session) {
    let throttle = 0;
    let steer = 0;
    const pressed = new Set();
    for (const source of session.inputSources) {
      const pad = source.gamepad;
      if (!pad) continue;
      const hand = source.handedness;
      const value = (i) => pad.buttons[i]?.value ?? 0;
      const down = pad.buttons.map((b) => b.pressed);
      const before = this.prev.get(hand) ?? [];
      const press = (i) => down[i] && !before[i];
      this.prev.set(hand, down);
      const stick = Math.abs(pad.axes[2] ?? 0) > 0.15 ? pad.axes[2] : 0;
      if (Math.abs(stick) > Math.abs(steer)) steer = stick;
      if (hand === 'right') {
        throttle += value(0);
        if (press(4)) pressed.add('fire');
        if (press(5)) pressed.add('flip');
      } else {
        throttle -= value(0);
        if (press(4)) pressed.add('camera');
        if (press(5)) pressed.add('pause');
      }
    }
    this.app.set_xr_input(throttle, steer, pressed.has('fire'), pressed.has('flip'), pressed.has('camera'));
    if (pressed.has('pause')) this.hooks.pause();
  }

  // The HUD panel: position, lap and weapon on the left and right, and the
  // countdown or other banner text in the middle. Repainted on change.
  paintHud() {
    const h = this.hooks.hud();
    const key = JSON.stringify(h);
    if (key === this.lastHud) return;
    this.lastHud = key;
    const g = this.hud.getContext('2d');
    g.clearRect(0, 0, HUD_W, HUD_H);
    // Pixels need alpha ≥ 0.5 to show (the shader's colour key), so the
    // panels are drawn nearly opaque and everything else stays clear.
    g.fillStyle = 'rgba(16, 19, 27, 0.85)';
    const panel = (x, w) => { g.beginPath(); g.roundRect(x, 8, w, HUD_H - 16, 24); g.fill(); };
    if (h.left.length) panel(8, 300);
    if (h.right.length) panel(HUD_W - 308, 300);
    g.textBaseline = 'middle';
    const lines = (list, x, align) => {
      g.textAlign = align;
      list.forEach((line, i) => {
        g.font = i === 0 ? 'bold 64px system-ui, sans-serif' : '40px system-ui, sans-serif';
        g.fillStyle = i === 0 ? '#f2b233' : '#e6e8ee';
        g.fillText(line, x, 70 + i * 64);
      });
    };
    lines(h.left, 40, 'left');
    lines(h.right, HUD_W - 40, 'right');
    if (h.banner) {
      g.textAlign = 'center';
      g.font = 'bold 120px system-ui, sans-serif';
      g.lineWidth = 12;
      g.strokeStyle = 'rgba(0, 0, 0, 0.9)';
      g.strokeText(h.banner, HUD_W / 2, HUD_H / 2);
      g.fillStyle = h.warn ? '#ff5a4a' : '#f2b233';
      g.fillText(h.banner, HUD_W / 2, HUD_H / 2);
    }
    this.app.set_xr_hud(this.hud);
  }
}
