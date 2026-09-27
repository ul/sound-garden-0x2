# sound_garden_web

Sound Garden in the browser: the compiler and VM compiled to WebAssembly, played by an AudioWorklet. See `docs/adr/0006-browser-engine.md` for the design.

```sh
make -C sound_garden_web          # build dist/: engine, worklet, page API, playground
make -C sound_garden_web serve    # playground on http://localhost:8124
```

Serve over http(s) or localhost; browsers don't load worklets from `file://`.

## GitHub Pages

Pushes to `master` build the WebAssembly playground with the locked Cargo dependencies and publish `dist/` via `.github/workflows/pages.yml`. You can also run the workflow manually from the Actions tab. The published directory includes the project manual at `README.html`.

## Playground

`dist/index.html`: type a program, commit with Cmd/Ctrl+Enter (edits keep op state, like the editor), Esc pauses. The modeline shows whether what you hear matches the text. "Copy link" puts the program, compressed, in the URL fragment: nothing leaves the browser until you send the link.

## Page API

```js
import { SoundGarden } from './sound-garden.js';

const garden = await SoundGarden.start();       // after a user gesture
const warnings = await garden.load('440 s 0.2 *'); // [{ index, message }], index = word position
garden.play();
garden.pause();
await garden.load(otherProgram, { fresh: true }); // unrelated program: don't carry state over
```
