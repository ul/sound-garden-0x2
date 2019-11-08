use audio_vm::{Op, Sample, Stack, CHANNELS};
use itertools::izip;
use std::collections::VecDeque;

pub struct Delay {
    // TODO VecDeque<Frame>
    buffers: Vec<VecDeque<Sample>>,
    mask: usize,
    sample_rate: Sample,
}

impl Delay {
    pub fn new(sample_rate: u32, max_delay: f64) -> Self {
        let sample_rate = Sample::from(sample_rate);
        // +1 because interpolation looks for the next sample
        // next_power_of_two to trade memory for speed by replacing `mod` with `&`
        let max_delay_frames = ((sample_rate * max_delay) as usize + 1).next_power_of_two();
        let mask = max_delay_frames - 1;
        let mut buffers = Vec::with_capacity(CHANNELS);
        for _ in 0..CHANNELS {
            let mut buffer = VecDeque::with_capacity(max_delay_frames);
            for _ in 0..max_delay_frames {
                buffer.push_front(0.0);
            }
            buffers.push(buffer);
        }
        Delay {
            buffers,
            mask,
            sample_rate,
        }
    }
}

impl Op for Delay {
    fn perform(&mut self, stack: &mut Stack) {
        let delay = stack.pop();
        let input = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (output, x, delay, buffer) in izip!(
            frame.iter_mut(),
            input.iter(),
            delay.iter(),
            self.buffers.iter_mut()
        ) {
            let z = delay * self.sample_rate;
            let delay = z as usize;
            let k = z.fract();
            let a = buffer[delay & self.mask];
            let b = buffer[(delay + 1) & self.mask];
            *output = (1.0 - k) * a + k * b;
            buffer.pop_back();
            buffer.push_front(*x);
        }
        stack.push(&frame);
    }
}
