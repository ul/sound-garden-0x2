//! # Phasor
//!
//! ```
//!  1     /|    /|    /|    /|
//!       / |   / |   / |   / |
//!  0   /  |  /  |  /  |  /  |
//!     /   | /   | /   | /   |
//! -1 /    |/    |/    |/    |
//! ```
//!
//! Phasor module generates a saw wave in the range -1..1.
//! Frequency is controlled by the input for each channel separately and can be variable.
//!
//! It is called phasor because it could be used as input phase for other oscillators, which become
//! just pure transformations then and are not required to care about handling varying frequency by
//! themselves anymore.
//!
//! Sources to connect: frequency.
use audio_vm::{Op, Sample, Stack, CHANNELS};
use itertools::izip;

#[derive(Clone)]
pub struct Phasor {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
}

impl Phasor {
    pub fn new(sample_rate: u32) -> Self {
        Phasor {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for Phasor {
    fn perform(&mut self, stack: &mut Stack) {
        for (phase, &frequency) in self.phases.iter_mut().zip(&stack.pop()) {
            let dx = frequency * self.sample_period;
            *phase = ((*phase + dx + 1.0) % 2.0) - 1.0;
        }
        stack.push(&self.phases);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct Phasor0 {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
}

impl Phasor0 {
    pub fn new(sample_rate: u32) -> Self {
        Phasor0 {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for Phasor0 {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let frequency = stack.pop();
        for (phase, &frequency, &phase0) in izip!(&mut self.phases, &frequency, &phase0) {
            let dx = frequency * self.sample_period;
            *phase = ((*phase + phase0 + dx + 1.0) % 2.0) - 1.0;
        }
        stack.push(&self.phases);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
