use audio_vm::{AtomicFrame, Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;
use std::sync::{atomic::Ordering, Arc};

pub struct TableReader {
    sample_rate: Sample,
    table: Arc<Vec<AtomicFrame>>,
}

impl TableReader {
    pub fn new(sample_rate: u32, table: Arc<Vec<AtomicFrame>>) -> Self {
        TableReader {
            sample_rate: Sample::from(sample_rate),
            table,
        }
    }
}

impl Op for TableReader {
    fn perform(&mut self, stack: &mut Stack) {
        let index = stack.pop();
        let mut frame = [0.0; CHANNELS];
        let size = self.table.len();
        for (channel, (sample, &ix)) in izip!(&mut frame, &index).enumerate() {
            let z = ix * self.sample_rate;
            let i = z as usize;
            let k = z.fract();
            let a = f64::from_bits(self.table[(i % size)][channel].load(Ordering::Relaxed));
            let b = f64::from_bits(self.table[(i + 1) % size][channel].load(Ordering::Relaxed));
            *sample = (1.0 - k) * a + k * b;
        }
        stack.push(&frame);
    }
}

pub struct TableWriter {
    frame: usize,
    last_trigger: Frame,
    table: Arc<Vec<AtomicFrame>>,
    trigger_frame: [usize; CHANNELS],
}

impl TableWriter {
    pub fn new(table: Arc<Vec<AtomicFrame>>) -> Self {
        TableWriter {
            frame: 0,
            last_trigger: [0.0; CHANNELS],
            table,
            trigger_frame: [0; CHANNELS],
        }
    }
}

impl Op for TableWriter {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let input = stack.peek();
        let size = self.table.len();
        for (channel, (&trigger, &input, last_trigger, trigger_frame)) in izip!(
            &trigger,
            &input,
            &mut self.last_trigger,
            &mut self.trigger_frame
        )
        .enumerate()
        {
            if *last_trigger <= 0.0 && trigger > 0.0 {
                *trigger_frame = self.frame;
            }
            let ix = self.frame - *trigger_frame;
            if ix < size {
                self.table[ix][channel].store(input.to_bits(), Ordering::Relaxed);
            }
            *last_trigger = trigger;
        }
        self.frame += 1;
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.frame = other.frame;
            self.last_trigger = other.last_trigger;
            self.trigger_frame = other.trigger_frame;
        }
    }
}
