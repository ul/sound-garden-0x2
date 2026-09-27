// Browser facade for the worklet engine. Serve over http(s) or localhost.
const here = new URL('.', import.meta.url);
const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Canonical ID; never round a persistent u64 through an unsafe Number. */
export function nodeId(value) {
  let n;
  if (typeof value === 'string' && /^[0-9a-f]{16}$/i.test(value)) n = BigInt(`0x${value}`);
  else if (typeof value === 'string' && /^(?:0x[0-9a-f]+|[0-9]+)$/i.test(value)) n = BigInt(value);
  else if (typeof value === 'bigint') n = value;
  else if (typeof value === 'number' && Number.isSafeInteger(value)) n = BigInt(value);
  else if (value && Number.isInteger(value.hi) && Number.isInteger(value.lo) &&
    value.hi >= 0 && value.hi <= 0xffffffff && value.lo >= 0 && value.lo <= 0xffffffff)
    n = (BigInt(value.hi) << 32n) | BigInt(value.lo);
  else throw new TypeError('Node ID must be a u64 hex/decimal string or {hi,lo}');
  if (n < 0n || n > 0xffffffffffffffffn) throw new RangeError('Node ID out of u64 range');
  return `0x${n.toString(16).padStart(16, '0')}`;
}

function warnings(report, nodes) {
  return report.split('\n').filter(Boolean).map((line) => {
    const tab = line.indexOf('\t');
    const key = line.slice(0, tab);
    const message = line.slice(tab + 1);
    if (!nodes) return { index: Number(key), message };
    const id = key === '-1' ? null : nodeId(BigInt(key));
    return { id, index: id === null ? -1 : nodes.findIndex((node) => node.id === id), message };
  });
}

function encodeNodes(nodes) {
  if (!Array.isArray(nodes)) throw new TypeError('nodes must be an array');
  const items = nodes.map(({ id, text }) => ({ id: nodeId(id), bytes: encoder.encode(text) }));
  const length = items.reduce((length, item) => length + 12 + item.bytes.length, 4);
  if (length > 0xffffffff || items.length > 0xffffffff) throw new RangeError('Project too large');
  const bytes = new Uint8Array(length);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, items.length, true);
  let offset = 4;
  for (const item of items) {
    view.setBigUint64(offset, BigInt(item.id), true);
    view.setUint32(offset + 8, item.bytes.length, true);
    bytes.set(item.bytes, offset + 12);
    offset += 12 + item.bytes.length;
  }
  return { bytes, nodes: items };
}

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
      numberOfInputs: 0, outputChannelCount: [2],
      processorOptions: { wasm, seed: (Math.random() * 2 ** 32) >>> 0 },
    });
    node.connect(context.destination);
    return new SoundGarden(context, node);
  }

  constructor(context, node) {
    this.context = context;
    this.node = node;
    this.playing = false;
    this.onmonitor = null;
    this.nextId = 0;
    this.waiting = new Map();
    this.generation = 0;
    this.warnings = [];
    this.midi = { supported: typeof navigator !== 'undefined' && !!navigator.requestMIDIAccess,
      connected: false, inputs: [] };
    node.port.onmessage = ({ data }) => {
      if (data.type === 'monitor') this.onmonitor?.({ ...data,
        midiDevice: this.midi.connected ? this.midi.inputs.join(', ') : null });
      if (data.type !== 'loaded') return;
      const pending = this.waiting.get(data.id);
      if (!pending) return;
      const report = warnings(decoder.decode(data.report), pending.nodes);
      this.warnings = report;
      this.generation = data.generation;
      pending.resolve(report);
      this.waiting.delete(data.id);
    };
  }

  /** Text playground: resolves to {index,message} warnings. */
  load(text, { fresh = false } = {}) {
    const bytes = encoder.encode(text);
    return this.#load({ type: 'load', text: bytes, fresh }, null, bytes.buffer);
  }

  /** Stable u64 IDs (hex strings or {hi,lo}); warnings {id,index,message}. */
  loadNodes(nodes, { fresh = false } = {}) {
    const encoded = encodeNodes(nodes);
    return this.#load({ type: 'loadNodes', bytes: encoded.bytes, fresh }, encoded.nodes, encoded.bytes.buffer);
  }

  #load(message, nodes, buffer) {
    const id = this.nextId++;
    const result = new Promise((resolve) => this.waiting.set(id, { resolve, nodes }));
    this.node.port.postMessage({ ...message, id }, [buffer]);
    return result;
  }

  setMonitor(id = '0x0') {
    this.node.port.postMessage({ type: 'monitorId', id: nodeId(id) });
  }

  setPatternMonitors(ids = []) {
    if (ids.length > 256) throw new RangeError('At most 256 pattern monitors');
    this.node.port.postMessage({ type: 'patternIds', ids: ids.map(nodeId) });
  }

  /** ~30Hz onmonitor: {scope:[L,R],samples:Float32Array interleaved LR,
   * patterns:[{id,value:[L,R]}],meters:{peak,rms,sampleRate,bufferFrames,
   * clipped,load,dropouts,loadAvailable,dropoutsAvailable,dropoutsEstimated},midiDevice,generation,playing}.
   * `clipped` is a window count; `dropouts` counts estimated over-budget
   * render quantums, not hardware underruns. Without a worklet monotonic timer,
   * `load` is null and `dropouts` is 0 with both availability flags false. */
  setOscilloscope(enabled) {
    this.node.port.postMessage({ type: 'oscilloscope', enabled: !!enabled });
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

  /** Opt-in Web MIDI 1.0 on all current inputs; unsupported/denied returns a status. */
  async connectMidi() {
    if (!this.midi.supported) return this.midi;
    try {
      const access = await navigator.requestMIDIAccess({ sysex: false });
      if (this.midiAccess) this.midiAccess.onstatechange = null;
      this.midiAccess = access;
      const update = () => {
        const inputs = [...access.inputs.values()].filter((input) => input.state === 'connected');
        for (const input of access.inputs.values()) input.onmidimessage = null;
        for (const input of inputs) input.onmidimessage = (event) => {
          const [status, data1, data2] = event.data;
          const kind = status & 0xf0;
          if (event.data.length < 3 || ![0x80, 0x90, 0xb0, 0xe0].includes(kind)) return;
          this.node.port.postMessage({ type: 'midi', kind: kind === 0x80 ? 0 : kind === 0x90 ? 1 : kind === 0xb0 ? 2 : 3,
            channel: status & 15, data1, data2 });
        };
        this.midi = { supported: true, connected: inputs.length > 0,
          inputs: inputs.map((input) => input.name || 'MIDI input') };
      };
      access.onstatechange = update;
      update();
    } catch (error) {
      this.midi = { supported: true, connected: false, inputs: [], error: String(error) };
    }
    return this.midi;
  }
}
