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
        let frame = [stack.pop()[self.channel]; CHANNELS];
        stack.push(&frame);
    }
}
