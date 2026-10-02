//! Waveform glides: continuing a cycle across oscillator types and morphing between shapes.
//!
//! Every oscillator runs its phase over -1..1 per cycle, so a phase means the same point in the
//! cycle to a sine, a triangle, a saw or a pulse. When a live edit replaces one oscillator with
//! another (`s` to `t`, `440 s` to `lfo s`), the new oscillator continues the old one's cycle, and
//! if the shape changed it crossfades from the old shape to its own over [`GLIDE_FRAMES`] at that
//! shared phase. Both shapes are continuous in time, so the morph is smooth however the shapes are
//! aligned. Rationale in docs/adr/0010-waveform-edits-morph.md.

use crate::glide::GLIDE_FRAMES;
use crate::phasor::{poly_blep_saw_sample, wrap_phase};
use crate::pulse::poly_blep_pulse_sample;
use crate::pure;
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

/// What an oscillator draws over one cycle of phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Sine,
    SineFast,
    Cosine,
    CosineFast,
    Triangle,
    Saw,
    Pulse,
}

impl Shape {
    /// The shape as a function of phase, for oscillators that draw it directly. A pulse needs
    /// its width, so it is drawn by the pulse oscillators themselves.
    pub fn function(self) -> fn(Sample) -> Sample {
        match self {
            Shape::Sine => pure::sine,
            Shape::SineFast => pure::sine_fast,
            Shape::Cosine => pure::cosine,
            Shape::CosineFast => pure::cosine_fast,
            Shape::Triangle => pure::triangle,
            Shape::Saw | Shape::Pulse => saw,
        }
    }

    /// The shape at `phase`. Saw and pulse edges are band-limited when `dt` (cycles per sample)
    /// is known; the triangle is drawn naively, its aliasing is negligible for the few
    /// milliseconds it is faded out.
    fn eval(self, phase: Sample, width: Sample, dt: Sample) -> Sample {
        match self {
            Shape::Pulse if dt != 0.0 => poly_blep_pulse_sample(phase, width, dt),
            Shape::Pulse => pure::rectangle(phase, width),
            Shape::Saw if dt != 0.0 => poly_blep_saw_sample(phase, dt),
            shape => shape.function()(phase),
        }
    }
}

fn saw(phase: Sample) -> Sample {
    phase
}

/// Where an oscillator is in its cycle and how it draws it: enough for another oscillator to
/// continue the cycle and fade this shape out.
#[derive(Clone, Copy, Debug)]
pub struct Cycle {
    pub(crate) phases: Frame,
    pub(crate) shape: Shape,
    /// Last phase offset input, for oscillators that take one (0 otherwise).
    pub(crate) phase0: Frame,
    /// Last pulse width input (0.5 for shapes without one).
    pub(crate) width: Frame,
    /// Last frequency in cycles per sample, for band-limited saws and pulses (0: draw naively).
    pub(crate) dt: Frame,
}

impl Cycle {
    pub(crate) fn new(phases: Frame, shape: Shape) -> Self {
        Cycle {
            phases,
            shape,
            phase0: [0.0; CHANNELS],
            width: [0.5; CHANNELS],
            dt: [0.0; CHANNELS],
        }
    }
}

/// Implemented by every oscillator, so [`cycle_of`] can find its cycle behind a `dyn Op`.
pub(crate) trait Oscillator {
    fn cycle(&self) -> Cycle;
}

/// The cycle of `op` if it is an oscillator.
pub(crate) fn cycle_of(op: &dyn Op) -> Option<Cycle> {
    use crate::{osc::*, phasor::*, pulse::*};
    macro_rules! try_types {
        ($($ty:ty),*) => {
            $(if let Some(osc) = op.downcast_ref::<$ty>() {
                return Some(osc.cycle());
            })*
        };
    }
    try_types!(
        Osc,
        FixedOsc,
        OscPhase,
        PolyBlepTriangle,
        PolyBlepTrianglePhase,
        Phasor,
        Phasor0,
        PolyBlepSawPhase,
        Pulse,
        PulsePhase,
        NaivePulse,
        NaivePulsePhase
    );
    None
}

/// An oscillator's frame without any morph: advance the cycle and draw it.
pub(crate) trait Draw {
    fn draw(&mut self, stack: &mut Stack) -> Frame;
    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame);
}

/// `Op::perform` for an oscillator. The common case is a straight-line leaf; a morph runs in a
/// cold function called in tail position, which needs no stack frame, so morphing costs nothing
/// except while it happens.
macro_rules! oscillator_perform {
    () => {
        fn perform(&mut self, stack: &mut Stack) {
            if self.morph.is_active() {
                return crate::waveform::perform_morphing(self, stack);
            }
            let frame = crate::waveform::Draw::draw(self, stack);
            stack.push(&frame);
        }
    };
}
pub(crate) use oscillator_perform;

#[cold]
#[inline(never)]
pub(crate) fn perform_morphing<O: Draw>(oscillator: &mut O, stack: &mut Stack) {
    let mut frame = oscillator.draw(stack);
    let (morph, phases) = oscillator.morph_and_phases();
    morph.step(phases, &mut frame);
    stack.push(&frame);
}

/// Fades an inherited shape out of an oscillator's output after an edit changed its shape.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Morph {
    from: Cycle,
    remaining: u32,
}

impl Morph {
    pub(crate) fn new() -> Self {
        Morph {
            from: Cycle::new([0.0; CHANNELS], Shape::Sine),
            remaining: 0,
        }
    }

    /// Start fading `previous`'s shape out if it differs from `shape`.
    pub(crate) fn start(&mut self, previous: &Cycle, shape: Shape) {
        if previous.shape != shape {
            self.from = *previous;
            self.remaining = GLIDE_FRAMES;
        }
    }

    #[inline(always)]
    pub(crate) fn is_active(&self) -> bool {
        self.remaining > 0
    }

    /// Blend the faded-out shape into `frame`, drawn at the oscillator's own `phases`.
    fn step(&mut self, phases: &Frame, frame: &mut Frame) {
        self.remaining -= 1;
        let progress = (GLIDE_FRAMES - self.remaining) as Sample / GLIDE_FRAMES as Sample;
        let old = 0.5 + 0.5 * (std::f64::consts::PI as Sample * progress).cos();
        for c in 0..CHANNELS {
            let from = &self.from;
            let previous = from.shape.eval(
                wrap_phase(phases[c] + from.phase0[c]),
                from.width[c],
                from.dt[c],
            );
            frame[c] = previous * old + frame[c] * (1.0 - old);
        }
    }
}
