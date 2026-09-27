// Sound Garden in the browser. One engine per page:
//
//   const garden = await SoundGarden.start();
//   const warnings = await garden.load('440 s 0.2 *');  // compile and crossfade, keeping state
//   garden.play(); garden.pause();
//
// Must be served over http(s) or localhost: browsers don't load worklets from file://.

const here = new URL('.', import.meta.url);

export class SoundGarden {
  static async start({ base = here } = {}) {
    const context = new AudioContext({ latencyHint: 'interactive' });
    const [wasm] = await Promise.all([
      fetch(new URL('sound_garden_web.wasm', base)).then((r) => {
        if (!r.ok) throw new Error(`Failed to fetch the engine: ${r.status}`);
        return r.arrayBuffer();
      }),
      context.audioWorklet.addModule(new URL('worklet.js', base)),
    ]);
    const node = new AudioWorkletNode(context, 'sound-garden', {
      numberOfInputs: 0,
      outputChannelCount: [2],
      processorOptions: { wasm, seed: (Math.random() * 2 ** 32) >>> 0 },
    });
    node.connect(context.destination);
    return new SoundGarden(context, node);
  }

  constructor(context, node) {
    this.context = context;
    this.node = node;
    this.playing = false;
    this.nextId = 0;
    this.waiting = new Map();
    node.port.onmessage = ({ data }) => {
      if (data.type !== 'loaded') return;
      const report = new TextDecoder().decode(data.report);
      const warnings = report
        .split('\n')
        .filter(Boolean)
        .map((line) => {
          const [index, message] = line.split('\t');
          return { index: Number(index), message };
        });
      this.waiting.get(data.id)?.(warnings);
      this.waiting.delete(data.id);
    };
  }

  /** Compile `text` and switch to it. Words that survive an edit keep their state; `fresh`
   *  starts from scratch (e.g. switching to an unrelated program). Resolves to warnings,
   *  each `{ index, message }` where index is the word position (-1: whole program). */
  load(text, { fresh = false } = {}) {
    const id = this.nextId++;
    const bytes = new TextEncoder().encode(text);
    this.node.port.postMessage({ type: 'load', id, text: bytes, fresh }, [bytes.buffer]);
    return new Promise((resolve) => this.waiting.set(id, resolve));
  }

  play() {
    this.context.resume();
    this.node.port.postMessage({ type: 'play', play: true });
    this.playing = true;
  }

  pause() {
    this.node.port.postMessage({ type: 'play', play: false });
    this.playing = false;
  }
}
