use crate::stack::Stack;
use downcast_rs::{Downcast, impl_downcast};

/// (Potentially stateful) instance of operation over Stack.
/// Corresponds to module/node in other systems.
pub trait Op: Send + Downcast {
    /// Perform operation with Stack.
    /// If Op was the last then top Frame from Stack will be sent to audio output.
    /// It must be called exactly once per audio frame.
    fn perform(&mut self, stack: &mut Stack);

    /// Transition from another Op.
    /// Implementations may copy small state or steal large state from the previous Op.
    /// Keep it efficient as it can block an audio thread.
    fn migrate(&mut self, _other: &mut dyn Op) {}

    /// The part of a pattern op's text sounding on the first channel, for the
    /// GUI's highlight. Called only for monitored statements, once per monitor interval.
    fn pattern_span(&self) -> PatternSpan {
        PatternSpan::default()
    }
}

/// Byte range `start..end` of a pattern's text; empty when nothing is sounding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatternSpan {
    pub start: u32,
    pub end: u32,
}

impl_downcast!(Op);
