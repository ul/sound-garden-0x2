// AudioWorklet processor hosting the Sound Garden engine (sound_garden_web.wasm).
// Programs arrive as UTF-8 bytes; the page encodes them, since worklets may lack TextEncoder.

class SoundGardenProcessor extends AudioWorkletProcessor {
  constructor({ processorOptions: { wasm, seed } }) {
    super();
    this.pending = [];
    this.port.onmessage = ({ data }) => (this.pending ? this.pending.push(data) : this.handle(data));
    WebAssembly.instantiate(wasm, {}).then(({ instance }) => {
      this.x = instance.exports;
      this.engine = this.x.sg_new(sampleRate, seed);
      this.max = this.x.sg_max_frames();
      for (const message of this.pending) this.handle(message);
      this.pending = null;
    });
  }

  handle(message) {
    const { x, engine } = this;
    switch (message.type) {
      case 'load': {
        if (message.fresh) x.sg_forget(engine);
        const text = message.text;
        new Uint8Array(x.memory.buffer, x.sg_input(engine, text.length), text.length).set(text);
        const length = x.sg_load(engine);
        const report = new Uint8Array(x.memory.buffer, x.sg_report(engine), length).slice();
        this.port.postMessage({ type: 'loaded', id: message.id, report }, [report.buffer]);
        break;
      }
      case 'play':
        x.sg_play(engine, message.play ? 1 : 0);
        break;
    }
  }

  process(_inputs, outputs) {
    if (!this.engine) return true;
    const [left, right] = outputs[0];
    const frames = left.length;
    const ptr = this.x.sg_process(this.engine, frames);
    // Views go stale when wasm memory grows (compiling can grow it), so re-create on change.
    if (this.view?.buffer !== this.x.memory.buffer || this.viewPtr !== ptr) {
      this.view = new Float32Array(this.x.memory.buffer, ptr, 2 * this.max);
      this.viewPtr = ptr;
    }
    left.set(this.view.subarray(0, frames));
    right?.set(this.view.subarray(this.max, this.max + frames));
    return true;
  }
}

registerProcessor('sound-garden', SoundGardenProcessor);
