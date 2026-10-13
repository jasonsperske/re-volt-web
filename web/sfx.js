// Sound effects from the disc's WAVS folder, driven by the engine's
// per-frame sound state (revs, skid, impact).

export class SoundFx {
  constructor(app, getContext) {
    this.app = app;
    this.getContext = getContext; // shared AudioContext, created on a user gesture
    this.buffers = new Map();
    this.loops = new Map(); // name -> { src, gain, path }
    this.master = null;
    this.enabled = true;
    this.lastHit = 0;
  }

  setEnabled(on) {
    this.enabled = on;
    if (this.master) this.master.gain.value = on ? 0.8 : 0;
  }

  #buffer(path) {
    if (!this.buffers.has(path)) {
      const ctx = this.getContext();
      this.buffers.set(path, this.app.read_game_file(path)
        .then((bytes) => ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength)))
        .catch(() => null));
    }
    return this.buffers.get(path);
  }

  #out() {
    const ctx = this.getContext();
    if (!this.master) {
      this.master = ctx.createGain();
      this.master.gain.value = this.enabled ? 0.8 : 0;
      this.master.connect(ctx.destination);
    }
    return this.master;
  }

  // Keeps a looping sound named `name` playing `path`; returns its nodes
  // once loaded (null until then).
  #loop(name, path) {
    const cur = this.loops.get(name);
    if (cur?.path === path) return cur.src ? cur : null;
    if (cur?.src) cur.src.stop();
    const entry = { path, src: null, gain: null };
    this.loops.set(name, entry);
    this.#buffer(path).then((buffer) => {
      if (!buffer || this.loops.get(name) !== entry) return;
      const ctx = this.getContext();
      entry.gain = ctx.createGain();
      entry.gain.gain.value = 0;
      entry.gain.connect(this.#out());
      entry.src = ctx.createBufferSource();
      entry.src.buffer = buffer;
      entry.src.loop = true;
      entry.src.connect(entry.gain);
      entry.src.start();
    });
    return null;
  }

  async #oneShot(path, volume) {
    const buffer = await this.#buffer(path);
    if (!buffer) return;
    const ctx = this.getContext();
    const gain = ctx.createGain();
    gain.gain.value = volume;
    gain.connect(this.#out());
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(gain);
    src.start();
  }

  play(path, volume = 1) {
    if (this.getContext()?.state === 'running') this.#oneShot(path, volume);
  }

  update(state) {
    const ctx = this.getContext();
    if (!ctx || ctx.state !== 'running' || !state) {
      for (const l of this.loops.values()) if (l.gain) l.gain.gain.value = 0;
      return;
    }
    const t = ctx.currentTime;
    const engine = this.#loop('engine', state.engine);
    if (engine) {
      const revs = Math.min(1.2, state.revs);
      engine.src.playbackRate.setTargetAtTime(0.5 + 1.3 * revs, t, 0.05);
      engine.gain.gain.setTargetAtTime(0.15 + 0.25 * Math.abs(state.throttle), t, 0.05);
    }
    const skid = this.#loop('skid', 'WAVS/SKID_NORMAL.WAV');
    if (skid) skid.gain.gain.setTargetAtTime(0.5 * state.skid * Math.min(1, state.speed / 800), t, 0.05);
    if (state.impact > 150 && t - this.lastHit > 0.15) {
      this.lastHit = t;
      this.play(state.impact > 600 ? 'WAVS/HIT2.WAV' : 'WAVS/HIT1.WAV', Math.min(1, state.impact / 1000));
    }
  }
}
