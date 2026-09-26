# MIDI controllers as smoothed signals

MIDI knobs, faders and the pitch-bend wheel are exposed as ordinary signals: `cc:N:DEFAULT` (controller N as `0..1`) and `bend` (`-1..1`), with primed raw forms `cc':N:DEFAULT` and `bend'`. They push one frame and consume nothing, like `param:N`, so they compose with every existing op (`cc:74 200 4000 uniexp`, `bend 2 * +` on a note number, inside `mpoly` bodies too).

Values live in a shared `MidiControls` store in the compile context, written by the audio callback at each message's timestamped frame (the same placement as notes, ADR 0003) and read by the ops. Keeping the value outside the op is what makes controls behave like hardware: a knob doesn't move when code is committed, so a newly compiled `cc` reads the current position, and ops added after the knob was turned still see it. Only notes go on the per-frame note bus; controller and bend messages bypass it, so `mpoly` is unaffected. Controls respond on any MIDI channel; channel filtering is deferred along with the rest of ADR 0003's channel work.

Until a control has received a message, the op outputs its default: `DEFAULT` for `cc` (clamped to `0..1`, 0 when omitted) and 0 for `bend`. This lets a patch sound as intended before anything is touched, and gives offline renders, which have no MIDI, a deterministic value.

The unprimed forms glide toward the latest value with a 10 ms one-pole so 7-bit steps don't zipper on filters and pitch. The raw forms output the stepped value, following the existing `'` convention for raw/naive variants. Both are one op type, so switching between them in a live edit keeps the current value. A new op starts at the current value instead of gliding up from zero, a recompiled op for the same control continues its glide through migration, and one for a different control starts fresh. A glide snaps to its target once within 1e-9 so it never decays into denormals.

Pitch bend is decoded from its 14-bit value with each side scaled separately so both extremes reach exactly -1 and 1. Invalid arguments (`cc`, `cc:128`, `cc:x`, a non-numeric default) push 0 with a warning, per the forgiveness convention.
