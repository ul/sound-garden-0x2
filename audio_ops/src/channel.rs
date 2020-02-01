use audio_vm::{Op, Stack, CHANNELS};

#[derive(Clone)]
pub struct Channel {
    channel: usize,
}

impl Channel {
    pub fn new(channel: usize) -> Self {
        Channel { channel }
    }
}

impl Op for Channel {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        frame[self.channel] = stack.pop()[self.channel];
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
