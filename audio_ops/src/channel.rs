use audio_vm::{Op, Stack, CHANNELS};

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
}
