# MIDI note bus and `mpoly`

## Status

Proposed.

## Context

Sound Garden currently has pattern-driven polyphony via `poly:N`: `<value> <ctl> poly:N` consumes a single serialized value/control lane, allocates on a rising control edge, latches the value, routes the live control signal to the current voice, and runs each voice body against `[latched_value, routed_ctl]`.

That design is excellent for sequenced patterns and one-lane triggers/gates, but it cannot represent normal keyboard chords. A single control signal cannot say "C's gate fell while E and G are still held". Proper MIDI therefore needs the deferred multi-lane generalization: independent note-on/note-off lifetimes per voice.

The current `adsr` op treats gate amplitude as a threshold/hold signal; it does not scale the envelope by gate amplitude. Therefore MIDI velocity must be exposed explicitly, not hidden only in the gate level.

Goal: plug a MIDI keyboard into standalone Sound Garden and write idiomatic programs such as:

```text
[ midi sine keys ] drop
[ 0.005 0.1 0.7 0.3 adsr rot m2f s * * ] mpoly:8
0.2 *
0.9 limit
```

The voice body starts with `[note, velocity, gate]`:

- `note`: latched MIDI note number, e.g. 60 for C4.
- `velocity`: latched normalized note-on velocity `0..1`.
- `gate`: per-voice gate, `1` while held and `0` after note-off. It is intentionally not velocity-scaled; use `velocity` explicitly for loudness/brightness.

The example above computes `adsr(gate, ...)`, rotates `[note, velocity, env]` to `[velocity, env, note]`, converts note to frequency, makes a sine, then multiplies by envelope and velocity.

## Decision

Add a MIDI-specific quotation consumer named `mpoly:N`.

```text
[ <voice-body> ] mpoly:N
```

`mpoly:N` owns `N` voices. Each active voice runs the body every sample against a stack seeded with:

```text
[note, velocity, gate]
```

The top frame after the body runs is that voice's audio output. `mpoly` sums all voice outputs and pushes exactly one frame. Empty or malformed `mpoly` compiles to a forgiving zero-output op that consumes no stack input and pushes silence.

`mpoly` does not consume stack inputs. MIDI is external I/O, analogous to `input` and `param:N`, but polyphonic and event-driven rather than scalar.

### Naming

Use `mpoly:N` rather than extending `poly:N` because:

- `poly:N`'s stack contract is already established as `<value> <ctl> poly:N`.
- MIDI has a different source of value/control data and requires note-off-aware allocation.
- A separate op keeps existing patches and migration semantics backward-compatible.
- The name is short enough for livecoding and visually parallel to `poly`.

Possible aliases such as `midipoly` can be added later, but `mpoly` is canonical.

## Voice allocation semantics

`mpoly` processes MIDI note events from a shared per-frame MIDI event source.

On note-on with velocity > 0:

1. Prefer a never-used voice.
2. Else prefer a released voice, oldest release first.
3. Else steal a held voice, oldest trigger first.

The selected voice latches:

- MIDI channel,
- MIDI note number,
- normalized velocity,
- gate = `1`.

On note-off, or note-on with velocity 0:

- Find a held voice with matching channel + note.
- Set its gate to `0` and mark it released.
- Keep note and velocity latched so the voice body can finish its release tail at the correct pitch and dynamics.

If multiple held voices have the same channel + note, release the oldest matching held voice. This gives deterministic behavior for repeated same-note presses.

If a held voice must be stolen, force a clean retrigger for edge-sensitive body ops such as `adsr`: route `gate = 0` for one sample, then route `gate = 1` on the following sample with the new note/velocity. This costs one sample of latency only on steals and avoids ADSR missing the new attack because its previous gate was already positive.

## Runtime architecture

### MIDI input thread

Use a cross-platform MIDI input crate such as `midir` for standalone MIDI input. The MIDI callback decodes raw MIDI bytes into small `MidiEvent` values and pushes them into a bounded lock-free ring buffer.

```rust
enum MidiEventKind { NoteOn, NoteOff }
struct MidiEvent {
    kind: MidiEventKind,
    channel: u8,      // 0..15
    note: u8,         // 0..127
    velocity: f64,    // 0..1, only meaningful for NoteOn
}
```

The MIDI callback must not allocate in the steady state. If the ring is full, drop the event and increment a dropped-event counter for diagnostics.

### Audio thread frame event source

The audio callback owns the ring consumer. At the start of each output frame, before `vm.next_frame()`, it drains queued MIDI events into a fixed-capacity per-frame event buffer shared with compiled `mpoly` ops. The buffer is non-consuming from the op perspective so multiple `mpoly` instances can respond to the same keyboard events.

Initial implementation may apply all drained events at the next audio frame. Sample-accurate MIDI timestamps inside an audio buffer are deferred.

The per-frame buffer must be fixed-capacity and real-time safe. A practical v1 design is:

- bounded SPSC ring from MIDI thread to audio callback, e.g. 1024 events,
- fixed `MAX_MIDI_EVENTS_PER_FRAME`, e.g. 64,
- extra drained events stay queued for following frames or are dropped with a counter if they exceed policy.

`mpoly` owns its own voice allocation state and reads the current frame's event slice each `perform`. Because events are not consumed by `mpoly`, multiple `mpoly` ops in one program work predictably.

### Program context

Add a MIDI source handle to `audio_program::Context`, similar in spirit to `input` and `params`:

```rust
pub struct Context {
    pub input: Arc<AtomicFrame>,
    pub params: [Arc<AtomicSample>; PARAMETERS],
    pub midi: Arc<MidiFrameEvents>,
    ...
}
```

`compile_mpoly` passes `Arc::clone(&ctx.midi)` into each `MPoly` op.

Programs compiled for `play_program` or `render_program` get an empty MIDI source; `mpoly` outputs silence offline unless a future render-time MIDI file/event source is provided.

## Live-edit migration

`MPoly::migrate` steals from the previous `MPoly` when the op type and node id match:

- voice allocation state: channel, note, velocity, gate, held/released state, trigger/release order, pending retrigger flags,
- per-voice body state by `(voice index, node id)` via existing `migrate_program_state`.

Growing `mpoly:N` preserves existing voices and adds inactive fresh voices. Shrinking drops highest-index voices. Held notes in surviving voices keep sounding across commits; body edits preserve oscillator/envelope/filter state just like top-level edits and existing `poly` bodies.

If a patch is edited from `poly:N` to `mpoly:N`, no allocator migration is attempted because the stack contract differs.

## Compiler changes

- Treat `mpoly` and `mpoly:<N>` as quotation consumers, parallel to `poly:N`.
- Add `compile_mpoly(op, body, sample_rate, ctx) -> MPoly`.
- `mpoly` without preceding quotation logs a warning and compiles to zero-output `MPoly::empty()`.
- Invalid `N`, `N == 0`, empty body, or malformed argument compile to zero-output with warning.
- Add help documentation under a new "MIDI" group or an expanded "Polyphony" group.

## Standalone UX

Initial deliverable should make the common case easy: plug in a keyboard, start Sound Garden, play.

Suggested CLI/UI shape:

```text
sound_garden_egui --midi auto my_tree.sg
sound_garden_egui --midi "Keystation" my_tree.sg
sound_garden_egui --list-midi

audio_server --midi auto
audio_server --midi "Keystation"
audio_server --list-midi
```

`auto` connects the first available MIDI input. A string selects by case-insensitive substring; a numeric string may select by index. The modeline should show MIDI status such as `midi: Keystation` or `midi: none`. Hotplug/reconnect can be deferred; restarting the app is acceptable for v1.

When `sound_garden_egui --audio-port ...` sends programs to an external `audio_server`, MIDI belongs to that external server. The GUI can still list local devices later, but v1 should document that MIDI flags matter only for the embedded server unless the server is launched with its own `--midi`.

## Example programs

### Sine keys

```text
[ midi sine keys ] drop
[ 0.005 0.1 0.7 0.3 adsr rot m2f s * * ] mpoly:8
0.2 *
0.9 limit
```

### Detuned triangle lead

```text
[ midi triangle lead note velocity gate ] drop
[ 0.005 0.1 0.7 0.25 adsr rot m2f dup t swap 2 * t 0.25 * + * * ] mpoly:6
0.22 *
=dry 4 0.5 verb 0.35 * <dry 0.7 * +
0.9 limit
```

### MIDI lead over sequenced bass

```text
[ midi over sequence play lead while bass runs ] drop
0.25 cycle >ph
<ph pat:A1,A1,C2,E2 0.04 lag m2f >f
<ph gate:x.xx 0.01 0.2 0.6 0.4 adsr >env
<f 2.01 * s <f * 0.8 * <env *
<f + s <env * 0.5 *
2 drive 0.7 *
0.45 3 comp:0.15 >bass
[ 0.005 0.1 0.7 0.25 adsr rot m2f dup t swap 2 * t 0.25 * + * * ] mpoly:6
0.22 *
<bass +
=dry 4 0.5 verb 0.35 * <dry 0.7 * +
0.9 limit
```

## Deferred

- Sustain pedal / CC64.
- Pitch bend.
- Mod wheel / arbitrary CC ops.
- Channel filtering and per-channel split/layer ops.
- Aftertouch, poly aftertouch, MPE.
- MIDI clock / transport sync.
- Hotplug/reconnect UI.
- Sample-accurate MIDI timestamps within audio buffers.
- VST/CLAP/AU host MIDI input. The current workspace does not contain a VST crate; standalone keyboard support comes first.
- MIDI file import/render support.

## Testing

Unit tests for `MPoly` should cover:

- note-on allocates a voice and seeds `[note, velocity, gate]`,
- note-off releases only the matching voice while other held chord notes continue,
- velocity is latched and available independently of gate,
- repeated same-note behavior,
- voice stealing and forced retrigger,
- migration preserves held notes and per-voice body state,
- growing/shrinking voice count,
- malformed/missing quotation compiles to zero-output.

Integration tests should compile and run the example programs against an injected synthetic MIDI event source without requiring hardware.
