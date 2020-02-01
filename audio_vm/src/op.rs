use crate::stack::Stack;

/// (Potentially stateful) instance of operation over Stack.
/// Corresponds to module/node in other systems.
pub trait Op: Send {
    /// Perform operation with Stack.
    /// If Op was the last then top Frame from Stack will be sent to audio output.
    /// It must be called exactly once per audio frame.
    fn perform(&mut self, stack: &mut Stack);

    /// Clone Op with current internal state.
    /// Keep it efficient as at the moment it can block an audio thread.
    /// It is used for cross-fading between two Programs sharing Ops.
    /// Not using Clone trait as it would prevent us from Op objects via Box<dyn Op>
    fn fork(&self) -> Box<dyn Op>;
}
