// CD-audio music, streamed straight from the disc's raw audio tracks.
//
// Red Book audio is 44.1 kHz, 16-bit little-endian stereo PCM with no
// headers, so tracks can be played by slicing the .bin files directly: a few
// seconds are read and scheduled at a time, never the whole track.

const SAMPLE_RATE = 44100;
const BYTES_PER_FRAME = 4; // 2 channels × 16-bit
const SECTOR = 2352;
const CHUNK_SECONDS = 2;
const AHEAD_SECONDS = 4;

// Maps CD track number -> { file, offset, length } using the cue sheet when
// one was selected, otherwise "(Track NN)" in the file names (one file per
// track, each starting at its own INDEX 01; files with a 2 s pregap are
// handled through the cue sheet only).
export async function findAudioTracks(files) {
  const byName = new Map([...files].map((f) => [f.name, f]));
  const tracks = new Map();
  const cue = [...files].find((f) => /\.cue$/i.test(f.name));
  if (cue) {
    let file = null;
    let track = null;
    for (const raw of (await cue.text()).split(/\r?\n/)) {
      const line = raw.trim();
      let m;
      if ((m = line.match(/^FILE\s+"(.+)"/i))) file = byName.get(m[1]) ?? null;
      else if ((m = line.match(/^TRACK\s+(\d+)\s+(\S+)/i))) track = m[2].toUpperCase() === 'AUDIO' ? Number(m[1]) : null;
      else if ((m = line.match(/^INDEX\s+01\s+(\d+):(\d+):(\d+)/i)) && file && track) {
        const sectors = (Number(m[1]) * 60 + Number(m[2])) * 75 + Number(m[3]);
        const offset = sectors * SECTOR;
        tracks.set(track, { file, offset, length: file.size - offset });
      }
    }
  }
  if (!tracks.size) {
    for (const f of files) {
      const m = f.name.match(/\(Track (\d+)\)\.bin$/i);
      if (m && Number(m[1]) > 1) tracks.set(Number(m[1]), { file: f, offset: 0, length: f.size });
    }
  }
  return tracks;
}

export class CdMusic {
  constructor() {
    this.ctx = null;
    this.gain = null;
    this.tracks = new Map();
    this.playlist = [];
    this.session = 0;
    this.volume = 0.6;
    this.enabled = true;
  }

  setTracks(tracks) {
    this.tracks = tracks;
  }

  get available() {
    return this.tracks.size > 0;
  }

  // Must be called from a user gesture at least once (browser autoplay rules).
  unlock() {
    if (!this.ctx) {
      this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
      this.gain = this.ctx.createGain();
      this.gain.gain.value = this.enabled ? this.volume : 0;
      this.gain.connect(this.ctx.destination);
    }
    if (this.ctx.state === 'suspended') this.ctx.resume();
  }

  setEnabled(on) {
    this.enabled = on;
    if (this.gain) this.gain.gain.setTargetAtTime(on ? this.volume : 0, this.ctx.currentTime, 0.1);
  }

  // As the game does: start at a random track in first..last, play through
  // to last, then loop the whole range from first.
  playRange(first, last) {
    const list = [];
    for (let t = first; t <= last; t++) if (this.tracks.has(t)) list.push(t);
    this.playlist = list;
    this.startIndex = Math.floor(Math.random() * list.length);
    this.stop();
    if (list.length) this.#run(++this.session);
  }

  stop() {
    this.session++;
    for (const src of this.sources ?? []) {
      try { src.stop(); } catch { /* already stopped */ }
    }
    this.sources = [];
  }

  async #run(session) {
    let index = this.startIndex ?? 0;
    while (session === this.session && this.playlist.length) {
      await this.#playTrack(this.playlist[index % this.playlist.length], session);
      index++;
    }
  }

  async #playTrack(number, session) {
    const { file, offset, length } = this.tracks.get(number);
    const chunkBytes = SAMPLE_RATE * CHUNK_SECONDS * BYTES_PER_FRAME;
    let when = 0;
    for (let pos = 0; pos < length && session === this.session; pos += chunkBytes) {
      // Wait until the browser has unlocked audio and the schedule needs topping up.
      while (session === this.session && (!this.ctx || this.ctx.state !== 'running' ||
             (when && when - this.ctx.currentTime > AHEAD_SECONDS))) {
        await new Promise((r) => setTimeout(r, 250));
      }
      if (session !== this.session) return;
      const end = Math.min(pos + chunkBytes, length);
      const bytes = await file.slice(offset + pos, offset + end).arrayBuffer();
      const pcm = new Int16Array(bytes, 0, Math.floor(bytes.byteLength / BYTES_PER_FRAME) * 2);
      const frames = pcm.length / 2;
      if (!frames || session !== this.session) continue;
      const buffer = this.ctx.createBuffer(2, frames, SAMPLE_RATE);
      const [l, r] = [buffer.getChannelData(0), buffer.getChannelData(1)];
      for (let i = 0; i < frames; i++) {
        l[i] = pcm[2 * i] / 32768;
        r[i] = pcm[2 * i + 1] / 32768;
      }
      const src = this.ctx.createBufferSource();
      src.buffer = buffer;
      src.connect(this.gain);
      when = Math.max(when, this.ctx.currentTime + 0.05);
      src.start(when);
      this.sources.push(src);
      src.onended = () => { this.sources = this.sources.filter((s) => s !== src); };
      when += buffer.duration;
    }
    // Let the last chunk finish before moving on.
    while (session === this.session && this.ctx && when - this.ctx.currentTime > 0.1) {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
}
