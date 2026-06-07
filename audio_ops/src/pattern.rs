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

#[derive(Clone, Debug, PartialEq)]
struct Cell<T> {
    start: Sample,
    end: Sample,
    value: T,
}

#[derive(Clone, Debug, PartialEq)]
enum PatternElement<T> {
    Atom(T),
    Group(Vec<PatternElement<T>>),
}

fn flatten_elements<T: Copy>(
    elements: &[PatternElement<T>],
    start: Sample,
    duration: Sample,
    cells: &mut Vec<Cell<T>>,
) {
    let step = duration / elements.len() as Sample;
    for (index, element) in elements.iter().enumerate() {
        let cell_start = start + step * index as Sample;
        let cell_end = if index + 1 == elements.len() {
            start + duration
        } else {
            cell_start + step
        };
        match element {
            PatternElement::Atom(value) => cells.push(Cell {
                start: cell_start,
                end: cell_end,
                value: *value,
            }),
            PatternElement::Group(group) => flatten_elements(group, cell_start, step, cells),
        }
    }
}

fn flatten_pattern<T: Copy>(elements: Vec<PatternElement<T>>) -> Vec<Cell<T>> {
    let mut cells = Vec::new();
    if !elements.is_empty() {
        flatten_elements(&elements, 0.0, 1.0, &mut cells);
    }
    cells
}

fn cell_index<T>(phase: Sample, cells: &[Cell<T>]) -> usize {
    let phase = wrap_phase(phase);
    cells
        .iter()
        .position(|cell| phase >= cell.start && phase < cell.end)
        .unwrap_or_else(|| cells.len().saturating_sub(1))
}

fn skip_ascii_whitespace(chars: &[char], cursor: &mut usize) {
    while chars
        .get(*cursor)
        .is_some_and(|ch| ch.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

fn parse_value_sequence(
    chars: &[char],
    cursor: &mut usize,
    in_group: bool,
) -> Option<Vec<PatternElement<Sample>>> {
    let mut elements = Vec::new();
    loop {
        skip_ascii_whitespace(chars, cursor);
        let Some(&ch) = chars.get(*cursor) else {
            return if in_group { None } else { Some(elements) };
        };
        if ch == ']' {
            if !in_group {
                return None;
            }
            *cursor += 1;
            return Some(elements);
        }
        if !elements.is_empty() {
            if ch != ',' {
                return None;
            }
            *cursor += 1;
            skip_ascii_whitespace(chars, cursor);
        }
        let Some(&ch) = chars.get(*cursor) else {
            return None;
        };
        match ch {
            '[' => {
                *cursor += 1;
                let group = parse_value_sequence(chars, cursor, true)?;
                if group.is_empty() {
                    return None;
                }
                elements.push(PatternElement::Group(group));
            }
            ']' => return None,
            _ => {
                let start = *cursor;
                while chars.get(*cursor).is_some_and(|ch| {
                    !ch.is_ascii_whitespace() && *ch != ',' && *ch != '[' && *ch != ']'
                }) {
                    *cursor += 1;
                }
                if start == *cursor {
                    return None;
                }
                let token: String = chars[start..*cursor].iter().collect();
                match token.parse::<Sample>() {
                    Ok(value) if value.is_finite() => elements.push(PatternElement::Atom(value)),
                    _ => return None,
                }
            }
        }
    }
}

fn parse_values(pattern: &str) -> Vec<Cell<Sample>> {
    let chars: Vec<_> = pattern.chars().collect();
    let mut cursor = 0;
    let elements = parse_value_sequence(&chars, &mut cursor, false).unwrap_or_default();
    let cells = flatten_pattern(elements);
    if cells.is_empty() {
        warn_invalid(ParseKind::Value, pattern);
    }
    cells
}

fn parse_gate_sequence(
    chars: &[char],
    cursor: &mut usize,
    in_group: bool,
) -> Option<Vec<PatternElement<bool>>> {
    let mut elements = Vec::new();
    loop {
        let Some(&ch) = chars.get(*cursor) else {
            return if in_group { None } else { Some(elements) };
        };
        match ch {
            'x' | 'X' => {
                elements.push(PatternElement::Atom(true));
                *cursor += 1;
            }
            '.' => {
                elements.push(PatternElement::Atom(false));
                *cursor += 1;
            }
            '[' => {
                *cursor += 1;
                let group = parse_gate_sequence(chars, cursor, true)?;
                if group.is_empty() {
                    return None;
                }
                elements.push(PatternElement::Group(group));
            }
            ']' => {
                if !in_group {
                    return None;
                }
                *cursor += 1;
                return Some(elements);
            }
            ch if ch.is_ascii_whitespace() || ch == ',' => {
                *cursor += 1;
            }
            _ => return None,
        }
    }
}

fn parse_gates(pattern: &str) -> Vec<Cell<bool>> {
    let chars: Vec<_> = pattern.chars().collect();
    let mut cursor = 0;
    let elements = parse_gate_sequence(&chars, &mut cursor, false).unwrap_or_default();
    let cells = flatten_pattern(elements);
    if cells.is_empty() {
        warn_invalid(ParseKind::Gate, pattern);
    }
    cells
}

#[derive(Clone)]
struct ValuePattern {
    values: Vec<Cell<Sample>>,
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
            *output = self.values[cell_index(phase, &self.values)].value;
        }
        frame
    }
}

#[derive(Clone)]
struct GatePattern {
    gates: Vec<Cell<bool>>,
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
            *output = if self.gates[cell_index(phase, &self.gates)].value {
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
            let index = cell_index(phase, &self.pattern.gates);
            let active = self.pattern.gates[index].value;
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
    fn value_pattern_subdivides_bracketed_groups() {
        let mut pat = PatternValue::new("60,[64,67],72,67");
        assert_eq!(perform(&mut pat, [0.0, 0.2499]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.3749]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.375, 0.4999]), [67.0, 67.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.75]), [72.0, 67.0]);
    }

    #[test]
    fn invalid_value_patterns_output_zero() {
        for pattern in ["", "60,,64", "abc", "NaN", "inf", "1e309", "60,[64"] {
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
    fn gate_pattern_subdivides_bracketed_groups() {
        let mut gate = PatternGate::new("x[x.]..");
        assert_eq!(perform(&mut gate, [0.0, 0.24]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.25, 0.3749]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.375, 0.5]), [0.0, 0.0]);
    }

    #[test]
    fn invalid_gate_patterns_output_zero() {
        for pattern in ["", "x..q", "1..0", "x[."] {
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
