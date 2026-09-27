import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const dist = process.argv[2];
if (!dist) throw new Error('Expected the dist directory');

// The entry point, generated glue and WASM share a cache key for this build.
const hash = createHash('sha256');
for (const path of [
  'editor/editor.js',
  'editor/pkg/sound_garden_egui.js',
  'editor/pkg/sound_garden_egui_bg.wasm',
]) {
  hash.update(path);
  hash.update(readFileSync(join(dist, path)));
}
const key = hash.digest('hex').slice(0, 24);
const page = join(dist, 'editor/index.html');
const html = readFileSync(page, 'utf8');
const placeholder = '__EDITOR_ASSET_KEY__';
if (html.split(placeholder).length !== 2) throw new Error('Expected one editor cache key placeholder');
writeFileSync(page, html.replace(placeholder, key));
console.log(`editor cache key: ${key}`);
