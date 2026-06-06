use audio_vm::{CHANNELS, Op, Stack};
use rand::{RngExt, rngs::SmallRng};

pub struct WhiteNoise {
    rng: SmallRng,
}

impl Default for WhiteNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl WhiteNoise {
    pub fn new() -> Self {
        WhiteNoise {
            rng: rand::make_rng(),
        }
    }
}

impl Op for WhiteNoise {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for sample in frame.iter_mut() {
            *sample = self.rng.random_range(-1.0..=1.0);
        }
        stack.push(&frame);
    }
}
