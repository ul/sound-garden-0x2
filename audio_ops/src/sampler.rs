use audio_vm::{AtomicFrame, CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;
use std::sync::{Arc, atomic::Ordering};

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
            let a = f64::from_bits(self.table[i % size][channel].load(Ordering::Relaxed));
            let b = f64::from_bits(self.table[(i + 1) % size][channel].load(Ordering::Relaxed));
            *sample = (1.0 - k) * a + k * b;
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>()
            && self.table.len() == other.table.len() {
                self.table = Arc::clone(&other.table);
            }
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

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            // Steal the live table Arc if sizes match — avoids wiping recorded content on reload.
            if self.table.len() == other.table.len() {
                self.table = Arc::clone(&other.table);
            }
            self.frame = other.frame;
            self.last_trigger = other.last_trigger;
            self.trigger_frame = other.trigger_frame;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(len: usize) -> Arc<Vec<AtomicFrame>> {
        let mut table = Vec::with_capacity(len);
        for _ in 0..len {
            table.push(Default::default());
        }
        Arc::new(table)
    }

    #[test]
    fn table_reader_migrates_live_table_arc() {
        let old_table = table(1);
        old_table[0][0].store(0.25f64.to_bits(), Ordering::Relaxed);
        old_table[0][1].store((-0.5f64).to_bits(), Ordering::Relaxed);
        let mut old_reader = TableReader::new(1, Arc::clone(&old_table));
        let mut new_reader = TableReader::new(1, table(1));

        new_reader.migrate(&mut old_reader);

        let mut stack = Stack::new();
        stack.push(&[0.0, 0.0]);
        new_reader.perform(&mut stack);
        assert_eq!(stack.peek(), [0.25, -0.5]);
    }

    #[test]
    fn table_writer_migrates_live_table_arc_and_cursor() {
        let old_table = table(1);
        let mut old_writer = TableWriter::new(Arc::clone(&old_table));
        let mut stack = Stack::new();
        stack.push(&[0.75, -0.25]);
        stack.push(&[1.0, 1.0]);
        old_writer.perform(&mut stack);
        let mut new_writer = TableWriter::new(table(1));

        new_writer.migrate(&mut old_writer);

        assert!(Arc::ptr_eq(&new_writer.table, &old_table));
        assert_eq!(new_writer.frame, 1);
        assert_eq!(new_writer.last_trigger, [1.0, 1.0]);
        assert_eq!(new_writer.trigger_frame, [0, 0]);
    }
}
