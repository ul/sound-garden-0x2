//! Compile diagnostics tied to the op (editor node) that caused them.
//!
//! Warnings are raised deep inside compilation (unknown words, bad op
//! arguments, invalid patterns). Rather than threading a context through every
//! op constructor, [`compile_warn!`] logs as before and, when a [`collect`] is
//! active on this thread, also records the message against the node id set by
//! the innermost [`OpScope`]. Compilation never runs on the audio thread.
use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    /// Node id of the op being compiled, if the warning came from one.
    pub id: Option<u64>,
    pub message: String,
}

thread_local! {
    static SINK: RefCell<Option<Vec<Diagnostic>>> = const { RefCell::new(None) };
    static CURRENT_OP: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Run `f`, returning its result and every diagnostic reported meanwhile.
/// Nested collects each see only their own diagnostics.
pub fn collect<R>(f: impl FnOnce() -> R) -> (R, Vec<Diagnostic>) {
    let outer = SINK.with(|sink| sink.replace(Some(Vec::new())));
    let result = f();
    let diagnostics = SINK.with(|sink| sink.replace(outer)).unwrap_or_default();
    (result, diagnostics)
}

/// Attribute diagnostics to node `id` until dropped; restores the enclosing
/// op's id, so container ops (poly bodies) attribute to the innermost op.
pub struct OpScope {
    previous: Option<u64>,
}

impl OpScope {
    pub fn enter(id: u64) -> Self {
        OpScope {
            previous: CURRENT_OP.with(|current| current.replace(Some(id))),
        }
    }
}

impl Drop for OpScope {
    fn drop(&mut self) {
        CURRENT_OP.with(|current| current.set(self.previous));
    }
}

/// Record `message` if a [`collect`] is active. Use [`compile_warn!`].
pub fn report(message: String) {
    SINK.with(|sink| {
        if let Some(diagnostics) = sink.borrow_mut().as_mut() {
            diagnostics.push(Diagnostic {
                id: CURRENT_OP.with(Cell::get),
                message,
            });
        }
    });
}

/// `log::warn!` that is also reported as a compile diagnostic for the
/// current op.
#[macro_export]
macro_rules! compile_warn {
    ($($arg:tt)*) => {{
        let message = format!($($arg)*);
        log::warn!("{}", message);
        $crate::diagnostics::report(message);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_attributed_to_the_innermost_op() {
        let ((), diagnostics) = collect(|| {
            compile_warn!("outside any op");
            let _outer = OpScope::enter(1);
            compile_warn!("in op {}", 1);
            {
                let _inner = OpScope::enter(2);
                compile_warn!("in a body op");
            }
            compile_warn!("back in op 1");
        });
        let seen = diagnostics
            .iter()
            .map(|d| (d.id, d.message.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            seen,
            [
                (None, "outside any op"),
                (Some(1), "in op 1"),
                (Some(2), "in a body op"),
                (Some(1), "back in op 1"),
            ]
        );
    }

    #[test]
    fn reports_outside_a_collect_are_only_logged() {
        compile_warn!("nobody is listening");
        let ((), diagnostics) = collect(|| ());
        assert!(diagnostics.is_empty());
    }
}
