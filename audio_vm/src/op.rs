use crate::stack::Stack;

pub trait Op {
    fn perform(&mut self, stack: &mut Stack);
}
