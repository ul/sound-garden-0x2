use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;
use std::collections::VecDeque;

pub struct Delay {
    buffer: VecDeque<Frame>,
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
        let mut buffer = VecDeque::with_capacity(max_delay_frames);
        for _ in 0..max_delay_frames {
            buffer.push_front([0.0; CHANNELS]);
        }
        Delay {
            buffer,
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
        for (channel, (output, delay)) in izip!(frame.iter_mut(), delay.iter(),).enumerate() {
            let z = delay * self.sample_rate;
            let delay = z as usize;
            let k = z.fract();
            let a = self.buffer[delay & self.mask][channel];
            let b = self.buffer[(delay + 1) & self.mask][channel];
            *output = (1.0 - k) * a + k * b;
        }
        stack.push(&frame);
        self.buffer.pop_back();
        self.buffer.push_front(input);
    }
}
