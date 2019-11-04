use audio_vm::{Op, Stack, CHANNELS};
use rand::{rngs::SmallRng, Rng, SeedableRng};

pub struct WhiteNoise {
    rng: SmallRng,
}

impl WhiteNoise {
    pub fn new() -> Self {
        WhiteNoise {
            rng: SmallRng::from_entropy(),
        }
    }
}

impl Op for WhiteNoise {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for sample in frame.iter_mut() {
            *sample = self.rng.gen_range(-1.0, 1.0);
        }
        stack.push(&frame);
    }
}
