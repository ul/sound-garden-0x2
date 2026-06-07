use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseKind {
    Value,
    Gate,
}

fn warn_invalid(kind: ParseKind, pattern: &str) {
    match kind {
        ParseKind::Value => log::warn!("Invalid numeric pattern: {}", pattern),
        ParseKind::Gate => log::warn!("Invalid gate pattern: {}", pattern),
    }
}

fn wrap_phase(phase: Sample) -> Sample {
    if phase.is_finite() {
        phase.rem_euclid(1.0)
    } else {
        0.0
    }
}

fn pattern_index(phase: Sample, len: usize) -> usize {
    let phase = wrap_phase(phase);
    let index = (phase * len as Sample).floor() as usize;
    index.min(len.saturating_sub(1))
}

fn parse_values(pattern: &str) -> Vec<Sample> {
    let mut values = Vec::new();
    if pattern.is_empty() {
        warn_invalid(ParseKind::Value, pattern);
        return values;
    }

    for token in pattern.split(',') {
        let token = token.trim();
        if token.is_empty() {
            warn_invalid(ParseKind::Value, pattern);
            return Vec::new();
        }
        match token.parse::<Sample>() {
            Ok(value) if value.is_finite() => values.push(value),
            _ => {
                warn_invalid(ParseKind::Value, pattern);
                return Vec::new();
            }
        }
    }

    values
}

fn parse_gates(pattern: &str) -> Vec<bool> {
    let mut gates = Vec::new();
    for ch in pattern.chars() {
        match ch {
            'x' | 'X' => gates.push(true),
            '.' => gates.push(false),
            ch if ch.is_ascii_whitespace() => {}
            _ => {
                warn_invalid(ParseKind::Gate, pattern);
                return Vec::new();
            }
        }
    }
    if gates.is_empty() {
        warn_invalid(ParseKind::Gate, pattern);
    }
    gates
}

#[derive(Clone)]
struct ValuePattern {
    values: Vec<Sample>,
}

impl ValuePattern {
    fn new(pattern: &str) -> Self {
        Self {
            values: parse_values(pattern),
        }
    }

    fn render(&self, phase: &Frame) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.values.is_empty() {
            return frame;
        }
        for (output, &phase) in frame.iter_mut().zip(phase) {
            *output = self.values[pattern_index(phase, self.values.len())];
        }
        frame
    }
}

#[derive(Clone)]
struct GatePattern {
    gates: Vec<bool>,
}

impl GatePattern {
    fn new(pattern: &str) -> Self {
        Self {
            gates: parse_gates(pattern),
        }
    }

    fn render_gate(&self, phase: &Frame) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.gates.is_empty() {
            return frame;
        }
        for (output, &phase) in frame.iter_mut().zip(phase) {
            *output = if self.gates[pattern_index(phase, self.gates.len())] {
                1.0
            } else {
                0.0
            };
        }
        frame
    }
}

pub struct Cycle {
    phases: Frame,
    sample_period: Sample,
}

impl Cycle {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }

    fn current_then_advance(&mut self, cps: &Frame) -> Frame {
        let phase = self.phases;
        for (phase, &cps) in self.phases.iter_mut().zip(cps) {
            let cps = if cps.is_finite() { cps } else { 0.0 };
            *phase = wrap_phase(*phase + cps * self.sample_period);
        }
        phase
    }
}

impl Op for Cycle {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.current_then_advance(&cps);
        stack.push(&phase);
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.phases = other.phases;
        }
    }
}

pub struct PatternValue {
    pattern: ValuePattern,
}

impl PatternValue {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: ValuePattern::new(pattern),
        }
    }
}

impl Op for PatternValue {
    fn perform(&mut self, stack: &mut Stack) {
        let phase = stack.pop();
        stack.push(&self.pattern.render(&phase));
    }
}

pub struct PatternGate {
    pattern: GatePattern,
}

impl PatternGate {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: GatePattern::new(pattern),
        }
    }
}

impl Op for PatternGate {
    fn perform(&mut self, stack: &mut Stack) {
        let phase = stack.pop();
        stack.push(&self.pattern.render_gate(&phase));
    }
}

pub struct PatternTrigger {
    pattern: GatePattern,
    previous_indices: [Option<usize>; CHANNELS],
    previous_phases: [Option<Sample>; CHANNELS],
}

impl PatternTrigger {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: GatePattern::new(pattern),
            previous_indices: [None; CHANNELS],
            previous_phases: [None; CHANNELS],
        }
    }

    fn render(&mut self, phase: &Frame) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.pattern.gates.is_empty() {
            return frame;
        }

        for (channel, (output, &phase)) in frame.iter_mut().zip(phase).enumerate() {
            let phase = wrap_phase(phase);
            let index = pattern_index(phase, self.pattern.gates.len());
            let active = self.pattern.gates[index];
            let entered_active_cell = self.previous_indices[channel] != Some(index) && active;
            let forward_cycle_wrap = self.previous_phases[channel].is_some_and(|prev| prev > phase);
            *output = if entered_active_cell || (forward_cycle_wrap && active) {
                1.0
            } else {
                0.0
            };
            self.previous_indices[channel] = Some(index);
            self.previous_phases[channel] = Some(phase);
        }

        frame
    }
}

impl Op for PatternTrigger {
    fn perform(&mut self, stack: &mut Stack) {
        let phase = stack.pop();
        let frame = self.render(&phase);
        stack.push(&frame);
    }
}

pub struct ClockedPatternValue {
    cycle: Cycle,
    pattern: ValuePattern,
}

impl ClockedPatternValue {
    pub fn new(sample_rate: u32, pattern: &str) -> Self {
        Self {
            cycle: Cycle::new(sample_rate),
            pattern: ValuePattern::new(pattern),
        }
    }
}

impl Op for ClockedPatternValue {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.cycle.current_then_advance(&cps);
        stack.push(&self.pattern.render(&phase));
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.cycle.phases = other.cycle.phases;
        }
    }
}

pub struct ClockedPatternGate {
    cycle: Cycle,
    pattern: GatePattern,
}

impl ClockedPatternGate {
    pub fn new(sample_rate: u32, pattern: &str) -> Self {
        Self {
            cycle: Cycle::new(sample_rate),
            pattern: GatePattern::new(pattern),
        }
    }
}

impl Op for ClockedPatternGate {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.cycle.current_then_advance(&cps);
        stack.push(&self.pattern.render_gate(&phase));
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.cycle.phases = other.cycle.phases;
        }
    }
}

pub struct ClockedPatternTrigger {
    cycle: Cycle,
    trigger: PatternTrigger,
}

impl ClockedPatternTrigger {
    pub fn new(sample_rate: u32, pattern: &str) -> Self {
        Self {
            cycle: Cycle::new(sample_rate),
            trigger: PatternTrigger::new(pattern),
        }
    }
}

impl Op for ClockedPatternTrigger {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.cycle.current_then_advance(&cps);
        let frame = self.trigger.render(&phase);
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.cycle.phases = other.cycle.phases;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut dyn Op, input: Frame) -> Frame {
        let mut stack = Stack::new();
        stack.push(&input);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn cycle_outputs_current_phase_then_advances_and_wraps() {
        let mut cycle = Cycle::new(4);
        assert_eq!(perform(&mut cycle, [1.0, -1.0]), [0.0, 0.0]);
        assert_eq!(perform(&mut cycle, [1.0, -1.0]), [0.25, 0.75]);
        assert_eq!(perform(&mut cycle, [f64::NAN, f64::INFINITY]), [0.5, 0.5]);
        assert_eq!(perform(&mut cycle, [2.0, -2.0]), [0.5, 0.5]);
    }

    #[test]
    fn value_pattern_selects_by_wrapped_phase_per_channel() {
        let mut pat = PatternValue::new("60, 64,67,72");
        assert_eq!(perform(&mut pat, [0.0, 0.2499]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.9999]), [64.0, 72.0]);
        assert_eq!(perform(&mut pat, [1.0, -0.25]), [60.0, 72.0]);
        assert_eq!(perform(&mut pat, [f64::NAN, f64::INFINITY]), [60.0, 60.0]);
    }

    #[test]
    fn invalid_value_patterns_output_zero() {
        for pattern in ["", "60,,64", "abc", "NaN", "inf", "1e309"] {
            let mut pat = PatternValue::new(pattern);
            assert_eq!(perform(&mut pat, [0.5, 0.5]), [0.0, 0.0]);
        }
    }

    #[test]
    fn gate_pattern_uses_dense_visual_notation() {
        let mut gate = PatternGate::new("x. X.");
        assert_eq!(perform(&mut gate, [0.0, 0.24]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.25, 0.5]), [0.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.75, 1.0]), [0.0, 1.0]);
    }

    #[test]
    fn invalid_gate_patterns_output_zero() {
        for pattern in ["", "x..q", "1..0"] {
            let mut gate = PatternGate::new(pattern);
            assert_eq!(perform(&mut gate, [0.0, 0.5]), [0.0, 0.0]);
        }
    }

    #[test]
    fn trigger_fires_on_initial_active_cell_entry_cell_changes_and_forward_wrap() {
        let mut trig = PatternTrigger::new("x.");
        assert_eq!(perform(&mut trig, [0.0, 0.5]), [1.0, 0.0]);
        assert_eq!(perform(&mut trig, [0.1, 0.75]), [0.0, 0.0]);
        assert_eq!(perform(&mut trig, [0.5, 0.0]), [0.0, 1.0]);
        assert_eq!(perform(&mut trig, [0.0, 0.25]), [1.0, 0.0]);
    }

    #[test]
    fn single_cell_trigger_fires_once_per_forward_cycle() {
        let mut trig = PatternTrigger::new("x");
        assert_eq!(perform(&mut trig, [0.0, 0.0]), [1.0, 1.0]);
        assert_eq!(perform(&mut trig, [0.5, 0.5]), [0.0, 0.0]);
        assert_eq!(perform(&mut trig, [0.0, 0.75]), [1.0, 0.0]);
    }

    #[test]
    fn clocked_pattern_value_matches_cycle_plus_value_pattern() {
        let mut clocked = ClockedPatternValue::new(4, "1,2,3,4");
        let mut cycle = Cycle::new(4);
        let mut pat = PatternValue::new("1,2,3,4");
        for _ in 0..8 {
            let clocked_frame = perform(&mut clocked, [1.0, 2.0]);
            let phase = perform(&mut cycle, [1.0, 2.0]);
            let explicit_frame = perform(&mut pat, phase);
            assert_eq!(clocked_frame, explicit_frame);
        }
    }
}
