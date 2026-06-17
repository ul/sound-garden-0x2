use audio_vm::{CHANNELS, Frame, Op, Sample, Stack, Statement, migrate_program_state};
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

pub const MAX_MIDI_EVENTS_PER_FRAME: usize = 64;
pub const MIDI_EVENT_RING_CAPACITY: usize = 1024;

const SILENCE: Frame = [0.0; CHANNELS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiEventKind {
    NoteOn,
    NoteOff,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiEvent {
    pub kind: MidiEventKind,
    pub channel: u8,
    pub note: u8,
    pub velocity: Sample,
}

impl MidiEvent {
    pub fn note_on(channel: u8, note: u8, velocity: Sample) -> Self {
        MidiEvent {
            kind: MidiEventKind::NoteOn,
            channel: channel.min(15),
            note: note.min(127),
            velocity: velocity.clamp(0.0, 1.0),
        }
    }

    pub fn note_off(channel: u8, note: u8) -> Self {
        MidiEvent {
            kind: MidiEventKind::NoteOff,
            channel: channel.min(15),
            note: note.min(127),
            velocity: 0.0,
        }
    }
}

struct AtomicMidiEvent {
    meta: AtomicU32,
    velocity: AtomicU64,
}

impl Default for AtomicMidiEvent {
    fn default() -> Self {
        AtomicMidiEvent {
            meta: AtomicU32::new(0),
            velocity: AtomicU64::new(0.0f64.to_bits()),
        }
    }
}

impl AtomicMidiEvent {
    fn store(&self, event: MidiEvent) {
        self.velocity
            .store(event.velocity.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        let kind = match event.kind {
            MidiEventKind::NoteOn => 1u32,
            MidiEventKind::NoteOff => 2u32,
        };
        let meta = kind | ((event.channel as u32) << 8) | ((event.note as u32) << 16);
        self.meta.store(meta, Ordering::Release);
    }

    fn load(&self) -> MidiEvent {
        let meta = self.meta.load(Ordering::Acquire);
        let kind = match meta & 0xff {
            1 => MidiEventKind::NoteOn,
            _ => MidiEventKind::NoteOff,
        };
        MidiEvent {
            kind,
            channel: ((meta >> 8) & 0xff) as u8,
            note: ((meta >> 16) & 0xff) as u8,
            velocity: Sample::from_bits(self.velocity.load(Ordering::Relaxed)),
        }
    }
}

/// Fixed-capacity per-audio-frame MIDI event slice.
///
/// The audio callback writes the current frame's drained MIDI events before
/// running the VM. `mpoly` ops read the same non-consuming slice, so multiple
/// `mpoly` instances can respond to the same keyboard events.
pub struct MidiFrameEvents {
    len: AtomicUsize,
    events: [AtomicMidiEvent; MAX_MIDI_EVENTS_PER_FRAME],
}

impl Default for MidiFrameEvents {
    fn default() -> Self {
        MidiFrameEvents {
            len: AtomicUsize::new(0),
            events: std::array::from_fn(|_| AtomicMidiEvent::default()),
        }
    }
}

impl MidiFrameEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_events(&self, events: &[MidiEvent]) {
        let len = events.len().min(MAX_MIDI_EVENTS_PER_FRAME);
        self.len.store(0, Ordering::Release);
        for (slot, &event) in self.events.iter().zip(events.iter()).take(len) {
            slot.store(event);
        }
        self.len.store(len, Ordering::Release);
    }

    pub fn clear(&self) {
        self.len.store(0, Ordering::Release);
    }

    pub fn copy_events(&self, out: &mut [MidiEvent; MAX_MIDI_EVENTS_PER_FRAME]) -> usize {
        let len = self
            .len
            .load(Ordering::Acquire)
            .min(MAX_MIDI_EVENTS_PER_FRAME);
        for (out, event) in out.iter_mut().zip(self.events.iter()).take(len) {
            *out = event.load();
        }
        len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceState {
    NeverUsed,
    Held,
    Released,
}

struct MVoice {
    program: Box<[Statement]>,
    channel: u8,
    note: u8,
    velocity: Sample,
    gate: Sample,
    state: VoiceState,
    trigger_order: u64,
    release_order: u64,
    pending_retrigger: bool,
}

pub struct MPoly {
    voices: Vec<MVoice>,
    midi: Arc<MidiFrameEvents>,
    order: u64,
    stack: Stack,
    event_buffer: [MidiEvent; MAX_MIDI_EVENTS_PER_FRAME],
}

impl MPoly {
    pub fn new(bodies: Vec<Box<[Statement]>>, midi: Arc<MidiFrameEvents>) -> Self {
        MPoly {
            voices: bodies
                .into_iter()
                .map(|program| MVoice {
                    program,
                    channel: 0,
                    note: 0,
                    velocity: 0.0,
                    gate: 0.0,
                    state: VoiceState::NeverUsed,
                    trigger_order: 0,
                    release_order: 0,
                    pending_retrigger: false,
                })
                .collect(),
            midi,
            order: 0,
            stack: Stack::new(),
            event_buffer: [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME],
        }
    }

    /// Forgiving zero-output op for invalid quotations/arguments. Unlike
    /// `poly`, `mpoly` consumes no stack input because MIDI is external I/O.
    pub fn empty(midi: Arc<MidiFrameEvents>) -> Self {
        Self::new(Vec::new(), midi)
    }

    fn next_order(&mut self) -> u64 {
        let order = self.order;
        self.order = self.order.wrapping_add(1);
        order
    }

    fn allocate_voice(&mut self) -> Option<usize> {
        if self.voices.is_empty() {
            return None;
        }
        if let Some(index) = self
            .voices
            .iter()
            .position(|voice| voice.state == VoiceState::NeverUsed)
        {
            return Some(index);
        }
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.state == VoiceState::Released)
            .min_by_key(|(_, voice)| voice.release_order)
        {
            return Some(index);
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, voice)| voice.trigger_order)
            .map(|(index, _)| index)
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: Sample) {
        let Some(index) = self.allocate_voice() else {
            return;
        };
        let was_held = self.voices[index].state == VoiceState::Held;
        let order = self.next_order();
        let voice = &mut self.voices[index];
        voice.channel = channel;
        voice.note = note;
        voice.velocity = velocity.clamp(0.0, 1.0);
        voice.trigger_order = order;
        voice.release_order = order;
        voice.state = VoiceState::Held;
        voice.gate = if was_held { 0.0 } else { voice.velocity };
        voice.pending_retrigger = was_held;
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| {
                voice.state == VoiceState::Held && voice.channel == channel && voice.note == note
            })
            .min_by_key(|(_, voice)| voice.trigger_order)
        else {
            return;
        };
        let order = self.next_order();
        let voice = &mut self.voices[index];
        voice.gate = 0.0;
        voice.state = VoiceState::Released;
        voice.release_order = order;
        voice.pending_retrigger = false;
    }

    fn process_events(&mut self) {
        for voice in &mut self.voices {
            if voice.pending_retrigger {
                voice.gate = voice.velocity;
                voice.pending_retrigger = false;
            }
        }

        let event_count = self.midi.copy_events(&mut self.event_buffer);
        for index in 0..event_count {
            let event = self.event_buffer[index];
            match event.kind {
                MidiEventKind::NoteOn if event.velocity > 0.0 => {
                    self.note_on(event.channel, event.note, event.velocity)
                }
                MidiEventKind::NoteOn | MidiEventKind::NoteOff => {
                    self.note_off(event.channel, event.note)
                }
            }
        }
    }
}

impl Op for MPoly {
    fn perform(&mut self, stack: &mut Stack) {
        self.process_events();
        let mut sum = SILENCE;
        for voice in &mut self.voices {
            if voice.state == VoiceState::NeverUsed {
                continue;
            }
            self.stack.reset();
            self.stack.push(&[voice.note as Sample; CHANNELS]);
            self.stack.push(&[voice.gate; CHANNELS]);
            for stmt in voice.program.iter_mut() {
                stmt.op.perform(&mut self.stack);
            }
            let frame = self.stack.peek();
            for (sum, x) in sum.iter_mut().zip(&frame) {
                *sum += x;
            }
        }
        stack.push(&sum);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.order = other.order;
            for (voice, other_voice) in self.voices.iter_mut().zip(other.voices.iter_mut()) {
                voice.channel = other_voice.channel;
                voice.note = other_voice.note;
                voice.velocity = other_voice.velocity;
                voice.gate = other_voice.gate;
                voice.state = other_voice.state;
                voice.trigger_order = other_voice.trigger_order;
                voice.release_order = other_voice.release_order;
                voice.pending_retrigger = other_voice.pending_retrigger;
                migrate_program_state(&mut voice.program, &mut other_voice.program);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe;

    impl Op for Probe {
        fn perform(&mut self, stack: &mut Stack) {
            let gate = stack.pop();
            let note = stack.pop();
            stack.push(&[note[0] + gate[0] * 100.0; CHANNELS]);
        }
    }

    struct Count {
        count: Sample,
    }

    impl Op for Count {
        fn perform(&mut self, stack: &mut Stack) {
            stack.pop();
            stack.pop();
            self.count += 1.0;
            stack.push(&[self.count; CHANNELS]);
        }

        fn migrate(&mut self, other: &mut dyn Op) {
            if let Some(other) = other.downcast_mut::<Self>() {
                self.count = other.count;
            }
        }
    }

    fn body(op: Box<dyn Op>) -> Box<[Statement]> {
        vec![Statement { id: 1, op }].into_boxed_slice()
    }

    fn probe_mpoly(voices: usize, midi: Arc<MidiFrameEvents>) -> MPoly {
        MPoly::new(
            (0..voices)
                .map(|_| body(Box::new(Probe) as Box<dyn Op>))
                .collect(),
            midi,
        )
    }

    fn frame(mpoly: &mut MPoly, midi: &MidiFrameEvents, events: &[MidiEvent]) -> Frame {
        midi.set_events(events);
        let mut stack = Stack::new();
        mpoly.perform(&mut stack);
        stack.peek()
    }

    #[test]
    fn note_on_allocates_and_seeds_note_and_velocity_gate() {
        let midi = Arc::new(MidiFrameEvents::new());
        let mut mpoly = probe_mpoly(2, Arc::clone(&midi));
        assert_eq!(
            frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 60, 0.7)]),
            [130.0, 130.0]
        );
    }

    #[test]
    fn note_off_releases_matching_voice_only() {
        let midi = Arc::new(MidiFrameEvents::new());
        let mut mpoly = probe_mpoly(2, Arc::clone(&midi));
        frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 60, 0.5)]);
        frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 64, 0.25)]);
        assert_eq!(
            frame(&mut mpoly, &midi, &[MidiEvent::note_off(0, 60)]),
            [149.0, 149.0]
        );
    }

    #[test]
    fn repeated_same_note_releases_oldest_matching_held_voice() {
        let midi = Arc::new(MidiFrameEvents::new());
        let mut mpoly = probe_mpoly(2, Arc::clone(&midi));
        frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 60, 0.5)]);
        frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 60, 0.25)]);
        assert_eq!(
            frame(&mut mpoly, &midi, &[MidiEvent::note_off(0, 60)]),
            [145.0, 145.0]
        );
    }

    #[test]
    fn stealing_forces_one_sample_retrigger() {
        let midi = Arc::new(MidiFrameEvents::new());
        let mut mpoly = probe_mpoly(1, Arc::clone(&midi));
        frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 60, 0.5)]);
        assert_eq!(
            frame(&mut mpoly, &midi, &[MidiEvent::note_on(0, 72, 0.8)]),
            [72.0, 72.0]
        );
        assert_eq!(frame(&mut mpoly, &midi, &[]), [152.0, 152.0]);
    }

    #[test]
    fn empty_mpoly_consumes_no_stack_input_and_pushes_silence() {
        let midi = Arc::new(MidiFrameEvents::new());
        let mut mpoly = MPoly::empty(midi);
        let mut stack = Stack::new();
        stack.push(&[5.0; CHANNELS]);
        mpoly.perform(&mut stack);
        assert_eq!(stack.pop(), [0.0, 0.0]);
        assert_eq!(stack.pop(), [5.0, 5.0]);
    }

    #[test]
    fn migrate_preserves_voice_and_body_state() {
        let midi = Arc::new(MidiFrameEvents::new());
        let count_poly = |voices: usize| {
            MPoly::new(
                (0..voices)
                    .map(|_| body(Box::new(Count { count: 0.0 }) as Box<dyn Op>))
                    .collect(),
                Arc::clone(&midi),
            )
        };
        let mut old = count_poly(1);
        frame(&mut old, &midi, &[MidiEvent::note_on(0, 60, 1.0)]);
        let mut new = count_poly(1);
        new.migrate(&mut old);
        assert_eq!(frame(&mut new, &midi, &[]), [2.0, 2.0]);
    }
}
