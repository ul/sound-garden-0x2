use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;
use std::sync::{Arc, Mutex};

pub struct TableReader {
    sample_rate: Sample,
    table: Arc<Mutex<Vec<Frame>>>,
}

impl TableReader {
    pub fn new(sample_rate: u32, table: Arc<Mutex<Vec<Frame>>>) -> Self {
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
        let table = self.table.lock().unwrap();
        let size = table.len();
        for (channel, (sample, &ix)) in izip!(&mut frame, &index).enumerate() {
            let z = ix * self.sample_rate;
            let i = z as usize;
            let k = z.fract();
            let a = table[(i % size)][channel];
            let b = table[(i + 1) % size][channel];
            *sample = (1.0 - k) * a + k * b;
        }
        stack.push(&frame);
    }
}

pub struct TableWriter {
    frame: usize,
    last_trigger: Frame,
    table: Arc<Mutex<Vec<Frame>>>,
    trigger_frame: [usize; CHANNELS],
}

impl TableWriter {
    pub fn new(table: Arc<Mutex<Vec<Frame>>>) -> Self {
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
        let mut table = self.table.lock().unwrap();
        let size = table.len();
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
                table[ix][channel] = input;
            }
            *last_trigger = trigger;
        }
        self.frame += 1;
    }
}
