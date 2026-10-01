# sound_garden_web

Sound Garden in the browser: the compiler and VM compiled to WebAssembly, played by an AudioWorklet. See `docs/adr/0007-browser-engine.md` for the design.

```sh
make -C sound_garden_web          # build dist/: both WASM modules, playground and editor
make -C sound_garden_web serve    # serve both at http://localhost:8124
```

Serve over http(s) or localhost; browsers don't load worklets from `file://`.

## GitHub Pages

Play at <https://ul.mantike.pro/sound-garden-0x2/> or open the [spatial editor](https://ul.mantike.pro/sound-garden-0x2/editor/).

Pushes to `master` build both WebAssembly interfaces with locked Cargo dependencies and publish `dist/` via `.github/workflows/pages.yml`. The directory includes the project manual at `README.html`.

## Playground

`dist/index.html`: type a program, commit with Cmd/Ctrl+Enter (edits keep op state), Esc pauses. "Copy link" puts the compressed program in the URL fragment. "Open in editor" transfers the current text into a separate spatial project.

## Spatial editor

`dist/editor/` runs the egui spatial editor in a browser canvas. Use Return to commit nodes to the AudioWorklet and the play control or backslash to start or pause sound. The modeline shows levels, program warnings, pattern output and scope; browser DSP-load and dropout counts are estimates when timing is available. Use Connect MIDI to grant browser MIDI input access where supported.

Projects are native Snappy/CBOR `.sg` documents. Import and export them with the toolbar. Named projects auto-save locally in IndexedDB, and the most recently opened project reopens after reload. The starter example becomes a local project after its first content edit. A project link embeds a small snapshot in the URL fragment; recipients open an independent copy. For larger projects use `.sg` export. If another tab updates the same local project, choose Reload newer version or Save a copy in the conflict dialog. If local storage is unavailable, export your edits before closing the page. Browser projects omit recording, path-backed samples and native TCP/CLI controls.
## Page API

```js
import { SoundGarden } from './sound-garden.js';

const garden = await SoundGarden.start();       // after a user gesture
const warnings = await garden.load('440 s 0.2 *'); // [{ index, message }], index = word position
garden.play();
garden.pause();
await garden.load(otherProgram, { fresh: true }); // unrelated program: don't carry state over
```
