// The browser's named projects are local copies of native .sg bytes. Revisions make
// simultaneous tabs detect conflicts instead of silently overwriting each other.
const DATABASE = 'sound-garden-editor';
const MAX_LINK_CHARS = 12000;

export class ConflictError extends Error {
  constructor() {
    super('This project was changed in another tab. Reload it or save a copy.');
    this.name = 'ConflictError';
  }
}

function requested(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function openProjectStore() {
  const request = indexedDB.open(DATABASE, 1);
  request.onupgradeneeded = () => {
    const db = request.result;
    db.createObjectStore('projects', { keyPath: 'id' });
    db.createObjectStore('meta', { keyPath: 'key' });
  };
  const db = await requested(request);
  return {
    async list() {
      return new Promise((resolve, reject) => {
        const projects = [];
        const cursor = db.transaction('projects').objectStore('projects').openCursor();
        cursor.onerror = () => reject(cursor.error);
        cursor.onsuccess = () => {
          if (!cursor.result) {
            resolve(projects.sort((a, b) => b.updatedAt - a.updatedAt));
            return;
          }
          const { id, name, revision, updatedAt } = cursor.result.value;
          projects.push({ id, name, revision, updatedAt });
          cursor.result.continue();
        };
      });
    },
    async get(id) {
      return requested(db.transaction('projects').objectStore('projects').get(id));
    },
    async activeId() {
      return (await requested(db.transaction('meta').objectStore('meta').get('active')))?.value ?? null;
    },
    async setActive(id) {
      return new Promise((resolve, reject) => {
        const tx = db.transaction('meta', 'readwrite');
        tx.objectStore('meta').put({ key: 'active', value: id });
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
    },
    // expectedRevision is null only for a new project. Copying the bytes keeps
    // an AudioWorklet or wasm memory growth from changing a queued save.
    async save({ id, name, bytes, expectedRevision }) {
      if (!id || !name?.trim() || !(bytes instanceof Uint8Array)) {
        throw new TypeError('Project id, name and .sg bytes are required');
      }
      return new Promise((resolve, reject) => {
        const tx = db.transaction('projects', 'readwrite');
        const store = tx.objectStore('projects');
        const lookup = store.get(id);
        let revision;
        lookup.onsuccess = () => {
          const previous = lookup.result;
          if ((previous?.revision ?? null) !== expectedRevision) {
            reject(new ConflictError());
            tx.abort();
            return;
          }
          revision = (expectedRevision ?? 0) + 1;
          store.put({
            id, name: name.trim(), revision, updatedAt: Date.now(),
            bytes: bytes.slice().buffer,
          });
        };
        tx.oncomplete = () => resolve(revision);
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error ?? new ConflictError());
      });
    },
    async remove(id, expectedRevision) {
      return new Promise((resolve, reject) => {
        const tx = db.transaction('projects', 'readwrite');
        const store = tx.objectStore('projects');
        const lookup = store.get(id);
        lookup.onsuccess = () => {
          if (lookup.result?.revision !== expectedRevision) {
            reject(new ConflictError());
            tx.abort();
          } else {
            store.delete(id);
          }
        };
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error ?? new ConflictError());
      });
    },
    close() { db.close(); },
  };
}

function base64url(bytes) {
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += 8192) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64url(code) {
  if (!/^[A-Za-z0-9_-]+$/.test(code) || code.length > MAX_LINK_CHARS) {
    throw new Error('Invalid or oversized project link');
  }
  const binary = atob(code.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - code.length % 4) % 4));
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

export function projectLink(bytes, editorUrl = new URL('./', location.href)) {
  const code = base64url(bytes);
  if (code.length > MAX_LINK_CHARS) return null; // export a .sg file instead
  return `${editorUrl.origin}${editorUrl.pathname}#project=${code}`;
}

export function projectFromHash(hash) {
  return hash.startsWith('#project=') ? fromBase64url(hash.slice(9)) : null;
}

export async function playgroundTextFromHash(hash) {
  if (!hash.startsWith('#text=')) return null;
  const compressed = fromBase64url(hash.slice(6));
  const reader = new Blob([compressed]).stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let text = '';
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 4_000_000) throw new Error('The shared program is too large');
      text += decoder.decode(value, { stream: true });
      if (text.length > 1_000_000) throw new Error('The shared program is too large');
    }
    text += decoder.decode();
    if (text.length > 1_000_000) throw new Error('The shared program is too large');
    return text;
  } finally {
    await reader.cancel().catch(() => {});
  }
}
