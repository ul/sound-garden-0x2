use crate::stack::Stack;
use downcast_rs::{impl_downcast, Downcast};

/// (Potentially stateful) instance of operation over Stack.
/// Corresponds to module/node in other systems.
pub trait Op: Send + Downcast {
    /// Perform operation with Stack.
    /// If Op was the last then top Frame from Stack will be sent to audio output.
    /// It must be called exactly once per audio frame.
    fn perform(&mut self, stack: &mut Stack);

    /// Smoothly transition from another Op.
    /// Usually just copy state from the Op of the same kind.
    /// Keep it efficient as it can block an audio thread.
    fn migrate(&mut self, _other: &Box<dyn Op>) {}
}

impl_downcast!(Op);
