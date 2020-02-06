use crate::buffer::Buffer;
use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

pub struct Delay {
    buffer: Buffer<Frame>,
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
        Delay {
            buffer: Buffer::new([0.0; CHANNELS], max_delay_frames),
            mask,
            sample_rate,
        }
    }

    pub fn migrate_same(&mut self, other: &Self) {
        self.buffer.copy_forward(&other.buffer);
    }
}

impl Op for Delay {
    fn perform(&mut self, stack: &mut Stack) {
        let delay = stack.pop();
        let input = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (channel, (output, delay)) in izip!(frame.iter_mut(), delay.iter()).enumerate() {
            let z = delay * self.sample_rate;
            let delay = z as usize;
            let k = z.fract();
            let a = self.buffer[delay & self.mask][channel];
            let b = self.buffer[(delay + 1) & self.mask][channel];
            *output = (1.0 - k) * a + k * b;
        }
        stack.push(&frame);
        self.buffer.push_front(input);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.migrate_same(other);
        }
    }
}

pub struct Prime {
    previous: Frame,
}

impl Prime {
    pub fn new() -> Self {
        Prime {
            previous: Default::default(),
        }
    }
}

impl Op for Prime {
    fn perform(&mut self, stack: &mut Stack) {
        let current = stack.pop();
        stack.push(&self.previous);
        self.previous = current;
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.previous = other.previous;
        }
    }
}
