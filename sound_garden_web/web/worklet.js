// AudioWorklet processor hosting the WASM engine. Messages arrive between render quanta.
class SoundGardenProcessor extends AudioWorkletProcessor {
  constructor({ processorOptions: { wasm, seed } }) {
    super();
    this.pending = [];
    this.enabled = false;
    this.playing = false;
    this.patternIds = [];
    this.framesSinceMonitor = 0;
    this.hasTimer = typeof performance !== 'undefined' && typeof performance.now === 'function';
    this.loadPeak = 0;
    this.dropouts = 0;
    this.port.onmessage = ({ data }) => (this.pending ? this.pending.push(data) : this.handle(data));
    WebAssembly.instantiate(wasm, {}).then(({ instance }) => {
      this.x = instance.exports;
      this.engine = this.x.sg_new(sampleRate, seed);
      this.max = this.x.sg_max_frames();
      for (const message of this.pending) this.handle(message);
      this.pending = null;
    });
  }

  write(bytes) {
    new Uint8Array(this.x.memory.buffer, this.x.sg_input(this.engine, bytes.length), bytes.length).set(bytes);
  }

  handle(message) {
    const { x, engine } = this;
    switch (message.type) {
      case 'load':
      case 'loadNodes': {
        if (message.fresh) {
          x.sg_forget(engine);
          if (message.type === 'loadNodes') x.sg_reset_program(engine);
        }
        this.write(message.type === 'load' ? message.text : message.bytes);
        const length = message.type === 'load' ? x.sg_load(engine) : x.sg_load_nodes(engine);
        const report = new Uint8Array(x.memory.buffer, x.sg_report(engine), length).slice();
        this.port.postMessage({ type: 'loaded', id: message.id, report,
          generation: x.sg_generation(engine) }, [report.buffer]);
        break;
      }
      case 'play':
        x.sg_play(engine, message.play ? 1 : 0);
        this.playing = !!message.play;
        break;
      case 'monitorId': {
        const id = BigInt(message.id);
        x.sg_set_monitor(engine, Number(id & 0xffffffffn), Number(id >> 32n));
        break;
      }
      case 'patternIds': {
        const ids = message.ids;
        const bytes = new Uint8Array(4 + ids.length * 8);
        const view = new DataView(bytes.buffer);
        view.setUint32(0, ids.length, true);
        ids.forEach((id, i) => view.setBigUint64(4 + i * 8, BigInt(id), true));
        this.write(bytes);
        x.sg_set_pattern_monitors(engine);
        this.patternIds = ids;
        break;
      }
      case 'oscilloscope':
        this.enabled = !!message.enabled;
        this.framesSinceMonitor = 0;
        x.sg_set_oscilloscope(engine, this.enabled ? 1 : 0);
        break;
      case 'midi':
        x.sg_midi(engine, message.kind, message.channel, message.data1, message.data2);
        break;
    }
  }

  // Peak render cost / quantum duration. An over-budget callback is a suspected
  // dropout; the browser does not expose hardware underrun counts to worklets.
  recordQuantum(started, frames) {
    if (started === null) return;
    const elapsed = performance.now() - started;
    const budget = frames * 1000 / sampleRate;
    if (!Number.isFinite(elapsed) || budget <= 0) return;
    const load = Math.max(0, elapsed / budget);
    this.loadPeak = Math.max(this.loadPeak, load);
    if (load > 1) this.dropouts++;
  }
  process(_inputs, outputs) {
    if (!this.engine) return true;
    const [left, right] = outputs[0];
    const frames = left.length;
    const started = this.hasTimer ? performance.now() : null;
    const ptr = this.x.sg_process(this.engine, frames);
    // Recreate views whenever compilation grows WASM memory.
    if (this.view?.buffer !== this.x.memory.buffer || this.viewPtr !== ptr) {
      this.view = new Float32Array(this.x.memory.buffer, ptr, 2 * this.max);
      this.viewPtr = ptr;
    }
    left.set(this.view.subarray(0, frames));
    right?.set(this.view.subarray(this.max, this.max + frames));
    if (this.enabled && (this.framesSinceMonitor += frames) >= sampleRate / 30) {
      this.framesSinceMonitor %= sampleRate / 30;
      const length = this.x.sg_capture_monitor(this.engine);
      const values = new Float32Array(this.x.memory.buffer, this.x.sg_monitor(this.engine), length);
      const count = this.x.sg_scope_frames(this.engine);
      const samples = new Float32Array(this.x.memory.buffer, this.x.sg_scope_samples(this.engine), count * 2).slice();
      this.x.sg_clear_scope(this.engine);
      const patterns = this.patternIds.map((id, i) => ({ id,
        value: [values[6 + 2 * i], values[7 + 2 * i]] }));
      this.recordQuantum(started, frames);
      const meters = { sampleRate, bufferFrames: frames,
        peak: [values[2], values[3]], rms: [values[4], values[5]],
        clipped: this.x.sg_take_clipped(this.engine),
        load: this.hasTimer ? this.loadPeak : null,
        dropouts: this.hasTimer ? this.dropouts : 0,
        loadAvailable: this.hasTimer, dropoutsAvailable: this.hasTimer,
        dropoutsEstimated: this.hasTimer };
      this.loadPeak = 0;
      this.port.postMessage({ type: 'monitor', scope: [values[0], values[1]], samples,
        patterns, meters, generation: this.x.sg_generation(this.engine),
        playing: this.playing }, [samples.buffer]);
    } else {
      this.recordQuantum(started, frames);
    }
    return true;
  }
}

registerProcessor('sound-garden', SoundGardenProcessor);
