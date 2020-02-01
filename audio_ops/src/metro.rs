use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

#[derive(Clone)]
pub struct Metro {
    last_trigger: [u64; CHANNELS],
    frame_number: u64,
    sample_rate: Sample,
}

impl Metro {
    pub fn new(sample_rate: u32) -> Self {
        Metro {
            last_trigger: [0; CHANNELS],
            frame_number: 0,
            sample_rate: Sample::from(sample_rate),
        }
    }
}

impl Op for Metro {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (output, &frequency, last_trigger) in
            izip!(&mut frame, &stack.pop(), &mut self.last_trigger)
        {
            let delta = self.sample_rate / frequency;
            *output = if delta as u64 <= self.frame_number - *last_trigger {
                *last_trigger = self.frame_number;
                1.0
            } else {
                0.0
            };
        }
        self.frame_number += 1;
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct DMetro {
    last_trigger: [u64; CHANNELS],
    frame_number: u64,
    sample_rate: Sample,
}

impl DMetro {
    pub fn new(sample_rate: u32) -> Self {
        DMetro {
            last_trigger: [0; CHANNELS],
            frame_number: 0,
            sample_rate: Sample::from(sample_rate),
        }
    }
}

impl Op for DMetro {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (output, &dt, last_trigger) in izip!(&mut frame, &stack.pop(), &mut self.last_trigger) {
            let delta = self.sample_rate * dt;
            *output = if delta as u64 <= self.frame_number - *last_trigger {
                *last_trigger = self.frame_number;
                1.0
            } else {
                0.0
            };
        }
        self.frame_number += 1;
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct MetroHold {
    frequencies: Frame,
    last_trigger: [u64; CHANNELS],
    frame_number: u64,
    sample_rate: Sample,
}

impl MetroHold {
    pub fn new(sample_rate: u32) -> Self {
        MetroHold {
            frequencies: [0.0; CHANNELS],
            last_trigger: [0; CHANNELS],
            frame_number: 0,
            sample_rate: Sample::from(sample_rate),
        }
    }
}

impl Op for MetroHold {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (output, &frequency, last_trigger, last_frequency) in izip!(
            &mut frame,
            &stack.pop(),
            &mut self.last_trigger,
            &mut self.frequencies
        ) {
            if *last_frequency == 0.0 {
                *last_frequency = frequency
            }
            let delta = self.sample_rate / *last_frequency;
            *output = if delta as u64 <= self.frame_number - *last_trigger {
                *last_trigger = self.frame_number;
                *last_frequency = frequency;
                1.0
            } else {
                0.0
            };
        }
        self.frame_number += 1;
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct DMetroHold {
    dts: Frame,
    last_trigger: [u64; CHANNELS],
    frame_number: u64,
    sample_rate: Sample,
}

impl DMetroHold {
    pub fn new(sample_rate: u32) -> Self {
        DMetroHold {
            dts: [0.0; CHANNELS],
            last_trigger: [0; CHANNELS],
            frame_number: 0,
            sample_rate: Sample::from(sample_rate),
        }
    }
}

impl Op for DMetroHold {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (output, &dt, last_trigger, last_dt) in izip!(
            &mut frame,
            &stack.pop(),
            &mut self.last_trigger,
            &mut self.dts
        ) {
            if *last_dt == 0.0 {
                *last_dt = dt
            }
            let delta = self.sample_rate * *last_dt;
            *output = if delta as u64 <= self.frame_number - *last_trigger {
                *last_trigger = self.frame_number;
                *last_dt = dt;
                1.0
            } else {
                0.0
            };
        }
        self.frame_number += 1;
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
