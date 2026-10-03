// Renders a program with the book's own renderer (book_render.wasm) and draws its figures, so
// the figures of a live edit match the printed ones exactly. Runs off the page's thread.
//
// In:  { id, program, seconds, show: ['wave' | 'spectrum' | 'spectrogram'], from, to, fmax, fscale }
// Out: { id, wave?: svg, spectrum?: svg, spectrogram?: { width, height, pixels: RGBA } }

const engine = fetch(new URL('book_render.wasm', import.meta.url))
  .then((response) => response.arrayBuffer())
  .then((bytes) => WebAssembly.instantiate(bytes, {}))
  .then(({ instance }) => instance.exports);

onmessage = async ({ data: job }) => {
  const x = await engine;
  // Calls can grow wasm memory, which detaches older views: always call first, then view.
  const bytes = new TextEncoder().encode(job.program);
  const input = x.br_input(bytes.length);
  new Uint8Array(x.memory.buffer, input, bytes.length).set(bytes);
  x.br_render(job.seconds);
  if (job.fade) x.br_fade(job.fade);
  const read = (length) => new Uint8Array(x.memory.buffer, x.br_output(), length).slice();
  const text = (length) => new TextDecoder().decode(read(length));

  const result = { id: job.id };
  if (job.show.includes('wave')) result.wave = text(x.br_wave(job.from ?? NaN, job.to ?? NaN));
  if (job.show.includes('spectrum')) {
    result.spectrum = text(x.br_spectrum(job.fmax ?? 20000, job.fscale === 'lin' ? 0 : 1));
  }
  if (job.show.includes('spectrogram')) {
    const pixels = read(x.br_spectrogram());
    result.spectrogram = { width: x.br_spectrogram_width(), height: x.br_spectrogram_height(), pixels };
  }
  postMessage(result, result.spectrogram ? [result.spectrogram.pixels.buffer] : []);
};
