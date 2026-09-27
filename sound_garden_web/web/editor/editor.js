import init, {
  start_sound_garden_editor,
  replace_sound_garden_project,
  current_sound_garden_project,
  empty_sound_garden_project,
  project_from_text,
} from './pkg/sound_garden_egui.js';
import { SoundGarden } from '../sound-garden.js';
import { ConflictError, openProjectStore, playgroundTextFromHash, projectFromHash, projectLink } from '../editor-store.js';

const STARTER = `[ welcome to the garden: edit, then enter to commit ] drop
2 s 1 + 220 * s
0.25 s 0.5 * 0.5 + *
0.2 *`;
const $ = (selector) => document.querySelector(selector);
const status = $('#status');
const canvas = $('#canvas');
let store = null;
let project = null; // {id, name, revision, starter}; null id means a new independent copy
let initialBytes = new Uint8Array();
let saves = Promise.resolve();
let pendingWrites = 0;
let blocked = false;
let unbackedEdits = false;
let garden = null;
let audioReady = null;
let playWanted = false;
let monitorId = '0000000000000000';
let patternIds = [];
let oscilloscope = false;
let diagnostics = { generation: 0, items: [] };
const monitorQueue = [];

function report(message) { status.textContent = message; }
function fail(error) { console.error(error); report(error?.message ?? String(error)); }
function showConflict() {
  blocked = true;
  report('Project changed in another tab. Reload it or save a copy before closing.');
  if (!$('#conflict-dialog').open) $('#conflict-dialog').showModal();
}
function nameFor(projectName) { return projectName.replace(/[\\/<>:"|?*\x00-\x1f]/g, '_').slice(0, 100) || 'Sound Garden'; }
function bytesNow() { return new Uint8Array(current_sound_garden_project()); }
async function drainSaves() {
  // A fresh edit may have joined the queue while an earlier save was pending.
  let current;
  do { current = saves; await current; } while (current !== saves);
}
function enqueue(frame) {
  monitorQueue.push(frame);
  if (monitorQueue.length > 24) monitorQueue.shift();
}

async function ensureAudio() {
  if (garden) return garden;
  if (!audioReady) {
    audioReady = SoundGarden.start().then((instance) => {
      garden = instance;
      garden.onmonitor = (frame) => {
          const samples = frame.samples ?? [];
          const stereo = [];
          for (let i = 0; i + 1 < samples.length && stereo.length < 512; i += 2) {
            stereo.push([samples[i], samples[i + 1]]);
          }
          enqueue({
            scope: frame.scope ?? [0, 0], samples: stereo,
            patterns: (frame.patterns ?? []).map(({ id, value }) => [id.replace(/^0x/, ''), value]),
              meters: {
                sampleRate: frame.meters?.sampleRate ?? instance.context.sampleRate,
                bufferFrames: frame.meters?.bufferFrames ?? 128,
                load: frame.meters?.load ?? null,
                loadAvailable: frame.meters?.loadAvailable ?? false,
                dropoutsEstimated: frame.meters?.dropoutsEstimated ?? false,
                peak: frame.meters?.peak ?? [0, 0],
                rms: frame.meters?.rms ?? [0, 0],
                clipped: frame.meters?.clipped ?? 0,
                dropouts: frame.meters?.dropouts ?? 0,
              },
            midiDevice: frame.midiDevice ?? null, diagnostics,
          });
        };
      instance.setMonitor?.(`0x${monitorId}`);
      instance.setPatternMonitors?.(patternIds.map((id) => `0x${id}`));
      instance.setOscilloscope?.(oscilloscope);
      return instance;
    }).catch((error) => { audioReady = null; fail(error); throw error; });
  }
  return audioReady;
}

async function persist(bytes, target = project) {
  if (!store || blocked) {
    if (!store) {
      unbackedEdits = true;
      report('Local storage unavailable. Export your project as .sg before closing.');
    }
    return;
  }
  const creating = !target.id;
  const id = target.id ?? crypto.randomUUID();
  const revision = await store.save({
    id, name: target.name, bytes, expectedRevision: target.revision,
  });
  target.id = id;
  target.revision = revision;
  target.starter = false;
  if (target === project) {
    if (creating) await store.setActive(id);
    unbackedEdits = false;
    if (location.hash) history.replaceState(null, '', location.pathname + location.search);
    report(`${target.name} · saved`);
  }
}

function saveProject(bytes, contentChanged = true) {
  if (project.starter && !contentChanged) return;
  const snapshot = new Uint8Array(bytes);
  const target = project;
  ++pendingWrites;
  saves = saves.then(() => persist(snapshot, target)).catch((error) => {
    if (error instanceof ConflictError) showConflict();
    else { unbackedEdits = true; fail(error); }
  }).finally(() => --pendingWrites);
}

window.soundGardenBridge = {
  dispatch(kind, payload) {
    try {
      const value = JSON.parse(payload);
      switch (kind) {
        case 'program':
          void ensureAudio().then((audio) => audio.loadNodes(value.map(({ id, text }) => ({ id: `0x${id}`, text })))).then((items) => {
            diagnostics = {
              generation: diagnostics.generation + 1,
              items: (items ?? []).map((item) => ({ ...item, id: item.id?.replace(/^0x/, '') ?? null })),
            };
            enqueue({ diagnostics });
          }).catch(fail);
          break;
          case 'play':
            playWanted = value;
            if (value) void ensureAudio().then((audio) => { if (playWanted) audio.play(); }).catch(fail);
            else garden?.pause();
          break;
        case 'monitor':
          monitorId = value;
          garden?.setMonitor?.(`0x${value}`);
          break;
        case 'patternMonitors':
          patternIds = value;
          garden?.setPatternMonitors?.(value.map((id) => `0x${id}`));
          break;
        case 'oscilloscope':
          oscilloscope = value;
          garden?.setOscilloscope?.(value);
          break;
        default: console.warn('Unknown editor command:', kind);
      }
    } catch (error) { fail(error); }
  },
  poll() { return JSON.stringify(monitorQueue.splice(0)); },
  loadProject() { return initialBytes; },
  saveProject,
};

async function changeProject(bytes, name, { save = false } = {}) {
  await drainSaves();
  if (unbackedEdits && !confirm('These edits are not saved locally. Export .sg before opening another project?')) return;
  if (blocked) return;
  replace_sound_garden_project(bytes); // decode first; corrupt files must leave the current project alone
  garden?.pause();
  playWanted = false;
  project = { id: null, revision: null, name, starter: false };
  if (save) await persist(new Uint8Array(bytes));
  else report(`${name} · unsaved copy`);
  canvas.focus();
}

async function listProjects() {
  const list = $('#project-list');
  list.replaceChildren();
  if (!store) {
    list.textContent = 'Local storage is unavailable. Use .sg export to keep your project.';
  } else {
    for (const item of await store.list()) {
      const row = document.createElement('div');
      const open = document.createElement('button');
      open.textContent = item.name;
      open.onclick = async () => {
        try {
            await drainSaves();
            if (unbackedEdits && !confirm('These edits are not saved locally. Export .sg before opening another project?')) return;
          if (blocked) return;
          const saved = await store.get(item.id);
          replace_sound_garden_project(new Uint8Array(saved.bytes));
          project = { id: saved.id, name: saved.name, revision: saved.revision, starter: false };
          await store.setActive(saved.id);
          history.replaceState(null, '', location.pathname + location.search);
          garden?.pause();
            playWanted = false;
          report(`${saved.name} · opened`);
          $('#project-dialog').close();
          canvas.focus();
        } catch (error) { fail(error); }
      };
      const rename = document.createElement('button');
      rename.textContent = 'Rename';
      rename.setAttribute('aria-label', `Rename ${item.name}`);
      rename.onclick = async () => {
        const name = prompt('Project name', item.name)?.trim();
        if (!name || name === item.name) return;
        try {
            await drainSaves();
          const saved = await store.get(item.id);
            if (!saved) throw new Error('Project was deleted in another tab');
            if (project.id === item.id && project.revision !== saved.revision) throw new ConflictError();
          const revision = await store.save({ id: item.id, name, bytes: new Uint8Array(saved.bytes), expectedRevision: saved.revision });
          if (project.id === item.id) { project.name = name; project.revision = revision; }
          await listProjects();
          } catch (error) { if (error instanceof ConflictError) showConflict(); else fail(error); }
      };
      const remove = document.createElement('button');
      remove.textContent = 'Delete';
      remove.setAttribute('aria-label', `Delete ${item.name}`);
      remove.onclick = async () => {
        if (!confirm(`Delete local project “${item.name}”? Export it first if needed.`)) return;
        try {
            await drainSaves();
          const saved = await store.get(item.id);
            if (!saved) throw new Error('Project was deleted in another tab');
            if (project.id === item.id && project.revision !== saved.revision) throw new ConflictError();
          await store.remove(item.id, saved.revision);
          if (project.id === item.id) {
            project = { id: null, name: 'Untitled', revision: null, starter: false };
            replace_sound_garden_project(empty_sound_garden_project());
            await store.setActive(null);
          }
          await listProjects();
          } catch (error) { if (error instanceof ConflictError) showConflict(); else fail(error); }
      };
      row.append(open, rename, remove);
      list.append(row);
    }
    if (!list.childElementCount) list.textContent = 'No saved projects yet.';
  }
  if (!$('#project-dialog').open) $('#project-dialog').showModal();
}

async function exportProject() {
  const bytes = bytesNow();
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${nameFor(project.name).replace(/\.sg$/i, '')}.sg`;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
  unbackedEdits = false;
  report(`${project.name} · exported`);
}

$('#projects').onclick = () => void listProjects().catch(fail);
$('#close-projects').onclick = () => $('#project-dialog').close();
$('#new').onclick = () => void changeProject(empty_sound_garden_project(), 'Untitled').catch(fail);
$('#import').onclick = () => $('#file').click();
$('#file').onchange = async ({ target }) => {
  const [file] = target.files;
  if (!file) return;
  try {
    if (file.size > 8 * 1024 * 1024) throw new Error('Project file is too large (8 MB limit)');
    await changeProject(new Uint8Array(await file.arrayBuffer()), nameFor(file.name), { save: true });
  } catch (error) { fail(error); }
  target.value = '';
};
$('#export').onclick = () => { try { void exportProject(); } catch (error) { fail(error); } };
$('#share').onclick = async () => {
  try {
    const link = projectLink(bytesNow());
    if (!link) { report('Project is too large for a link; use Export .sg.'); return; }
    history.replaceState(null, '', link);
    try { await navigator.clipboard.writeText(link); report('Project link copied'); }
    catch { report('Project link is in the address bar'); }
  } catch (error) { fail(error); }
};
$('#midi').onclick = async () => {
  try {
    const audio = await ensureAudio();
    const result = await audio.connectMidi();
    if (result.error) throw new Error(result.error);
    if (!result.supported) report('Web MIDI is unavailable in this browser');
    else if (!result.connected) report('MIDI access was not granted');
    else report(result.inputs?.length ? `MIDI connected: ${result.inputs.join(', ')}` : 'MIDI enabled; no input devices found');
  } catch (error) { fail(error); }
};
if (!navigator.requestMIDIAccess) {
  $('#midi').disabled = true;
  $('#midi').title = 'Web MIDI is unavailable in this browser';
}
$('#reload-project').onclick = async () => {
  try {
    const latest = await store.get(project.id);
    if (!latest) throw new Error('Project was deleted in another tab; save a copy instead.');
    replace_sound_garden_project(new Uint8Array(latest.bytes));
    project.revision = latest.revision;
    blocked = false;
    $('#conflict-dialog').close();
    report(`${project.name} · reloaded`);
  } catch (error) { fail(error); }
};
$('#save-copy').onclick = async () => {
  try {
    const bytes = bytesNow();
    project = { id: null, name: `${project.name} copy`, revision: null, starter: false };
    blocked = false;
    await persist(bytes);
    $('#conflict-dialog').close();
  } catch (error) { fail(error); }
};
window.addEventListener('beforeunload', (event) => {
    if (!blocked && pendingWrites === 0 && !unbackedEdits) return;
  event.preventDefault();
  event.returnValue = '';
});

try {
  try { store = await openProjectStore(); }
  catch (error) { console.error('Local project storage unavailable:', error); }
  let incoming = null;
  let incomingText = null;
  try {
    incoming = projectFromHash(location.hash);
    incomingText = await playgroundTextFromHash(location.hash);
  } catch (error) { fail(error); }
  await init();
  if (incomingText !== null) incoming = project_from_text(incomingText);
  if (incoming) {
    initialBytes = incoming;
    project = { id: null, name: incomingText === null ? 'Shared project' : 'Imported program', revision: null, starter: false };
  } else {
    const active = store && await store.activeId();
    const saved = active && await store.get(active);
    if (saved) {
      initialBytes = new Uint8Array(saved.bytes);
      project = { id: saved.id, name: saved.name, revision: saved.revision, starter: false };
    } else {
      initialBytes = project_from_text(STARTER);
      project = { id: null, name: 'Garden 1', revision: null, starter: true };
    }
  }
  await start_sound_garden_editor('canvas');
  report(`${project.name}${project.starter ? ' · example (make an edit to save your copy)' : project.id ? ' · ready' : ' · unsaved copy'}`);
  canvas.focus();
} catch (error) {
  fail(error);
}
