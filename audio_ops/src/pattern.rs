use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use nom::IResult;
use nom::Parser;
use nom::branch::alt;
use nom::bytes::complete::take_while1;
use nom::character::complete::{char, digit1, multispace0};
use nom::combinator::{all_consuming, map, map_res, opt, value};
use nom::multi::{many0, separated_list1};
use nom::sequence::{delimited, preceded, terminated, tuple};

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
    Alternate(Vec<Vec<PatternElement<T>>>),
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn lcm(a: usize, b: usize) -> usize {
    if a == 0 || b == 0 {
        0
    } else {
        a / gcd(a, b) * b
    }
}

fn element_period<T>(element: &PatternElement<T>) -> usize {
    match element {
        PatternElement::Atom(_) => 1,
        PatternElement::Group(elements) => pattern_period(elements),
        PatternElement::Alternate(alternatives) => alternatives
            .iter()
            .fold(alternatives.len(), |period, alternative| {
                lcm(period, pattern_period(alternative))
            }),
    }
}

fn pattern_period<T>(elements: &[PatternElement<T>]) -> usize {
    elements
        .iter()
        .fold(1, |period, element| lcm(period, element_period(element)))
}

fn flatten_elements<T: Copy>(
    elements: &[PatternElement<T>],
    cycle: usize,
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
            PatternElement::Group(group) => flatten_elements(group, cycle, cell_start, step, cells),
            PatternElement::Alternate(alternatives) => {
                let alternative = &alternatives[cycle % alternatives.len()];
                flatten_elements(alternative, cycle, cell_start, step, cells);
            }
        }
    }
}

fn flatten_pattern<T: Copy>(elements: &[PatternElement<T>], cycle: usize) -> Vec<Cell<T>> {
    let mut cells = Vec::new();
    if !elements.is_empty() {
        flatten_elements(elements, cycle, 0.0, 1.0, &mut cells);
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

#[derive(Clone)]
struct Pattern<T> {
    variants: Vec<Vec<Cell<T>>>,
}

impl<T> Pattern<T> {
    fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    fn cells(&self, cycle: usize) -> &[Cell<T>] {
        &self.variants[cycle % self.variants.len()]
    }
}

fn compile_pattern<T: Copy>(elements: Vec<PatternElement<T>>) -> Pattern<T> {
    let period = pattern_period(&elements).max(1);
    let variants = (0..period)
        .map(|cycle| flatten_pattern(&elements, cycle))
        .collect::<Vec<_>>();
    if variants.iter().any(Vec::is_empty) {
        Pattern {
            variants: Vec::new(),
        }
    } else {
        Pattern { variants }
    }
}

fn update_cycle_counts(
    phase: &Frame,
    previous_phases: &mut [Option<Sample>; CHANNELS],
    cycle_counts: &mut [usize; CHANNELS],
) {
    for (channel, &phase) in phase.iter().enumerate() {
        let phase = wrap_phase(phase);
        if previous_phases[channel].is_some_and(|prev| prev > phase) {
            cycle_counts[channel] = cycle_counts[channel].wrapping_add(1);
        }
        previous_phases[channel] = Some(phase);
    }
}

fn ws<'a, F, O>(parser: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: Parser<&'a str, O, nom::error::Error<&'a str>>,
{
    delimited(multispace0, parser, multispace0)
}

fn unsigned(input: &str) -> IResult<&str, usize> {
    map_res(digit1, str::parse::<usize>)(input)
}

fn repeat_suffix(input: &str) -> IResult<&str, usize> {
    map(opt(preceded(char('*'), unsigned)), |repeat| {
        repeat.unwrap_or(1)
    })(input)
}

fn positive_repeat(input: &str) -> IResult<&str, usize> {
    let (input, repeat) = repeat_suffix(input)?;
    if repeat == 0 {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )))
    } else {
        Ok((input, repeat))
    }
}

fn repeat_elements<T: Clone>(element: PatternElement<T>, repeat: usize) -> Vec<PatternElement<T>> {
    (0..repeat).map(|_| element.clone()).collect()
}

fn euclidean_values<T: Copy>(
    pulses: usize,
    steps: usize,
    on: T,
    off: T,
) -> Option<Vec<PatternElement<T>>> {
    if pulses > steps || steps == 0 {
        return None;
    }
    Some(
        (0..steps)
            .map(|step| {
                PatternElement::Atom(if (step * pulses) % steps < pulses {
                    on
                } else {
                    off
                })
            })
            .collect(),
    )
}

fn euclidean_args(input: &str) -> IResult<&str, (usize, usize)> {
    delimited(
        char('('),
        map(
            tuple((ws(unsigned), char(','), ws(unsigned))),
            |(pulses, _, steps)| (pulses, steps),
        ),
        char(')'),
    )(input)
}

fn value_atom(input: &str) -> IResult<&str, PatternElement<Option<Sample>>> {
    alt((
        value(PatternElement::Atom(None), char('_')),
        map_res(
            tuple((
                take_while1(|ch: char| {
                    !ch.is_ascii_whitespace()
                        && ch != ','
                        && ch != '['
                        && ch != ']'
                        && ch != '<'
                        && ch != '>'
                        && ch != ';'
                        && ch != '*'
                        && ch != '('
                }),
                opt(euclidean_args),
            )),
            |(token, euclid): (&str, Option<(usize, usize)>)| {
                let value = token.parse::<Sample>().map_err(|_| ())?;
                if !value.is_finite() {
                    return Err(());
                }
                Ok(match euclid {
                    Some((pulses, steps)) => PatternElement::Group(
                        euclidean_values(pulses, steps, Some(value), None).ok_or(())?,
                    ),
                    None => PatternElement::Atom(Some(value)),
                })
            },
        ),
    ))(input)
}

fn value_group(input: &str) -> IResult<&str, PatternElement<Option<Sample>>> {
    map(
        delimited(char('['), value_sequence, char(']')),
        PatternElement::Group,
    )(input)
}

fn value_alternate(input: &str) -> IResult<&str, PatternElement<Option<Sample>>> {
    map(
        delimited(
            char('<'),
            separated_list1(char(';'), value_sequence),
            char('>'),
        ),
        PatternElement::Alternate,
    )(input)
}

fn value_item(input: &str) -> IResult<&str, Vec<PatternElement<Option<Sample>>>> {
    let (input, element) = ws(alt((value_group, value_alternate, value_atom)))(input)?;
    let (input, repeat) = positive_repeat(input)?;
    Ok((input, repeat_elements(element, repeat)))
}

fn value_sequence(input: &str) -> IResult<&str, Vec<PatternElement<Option<Sample>>>> {
    map(separated_list1(char(','), value_item), |items| {
        items.into_iter().flatten().collect()
    })(input)
}

fn resolve_value_holds(cells: Vec<Cell<Option<Sample>>>) -> Vec<Cell<Sample>> {
    let Some(mut held) = cells.iter().rev().find_map(|cell| cell.value) else {
        return Vec::new();
    };
    cells
        .into_iter()
        .map(|cell| {
            if let Some(value) = cell.value {
                held = value;
            }
            Cell {
                start: cell.start,
                end: cell.end,
                value: held,
            }
        })
        .collect()
}

fn parse_values(pattern: &str) -> Pattern<Sample> {
    let parsed = all_consuming(terminated(value_sequence, multispace0))(pattern);
    match parsed {
        Ok((_, elements)) => {
            let period = pattern_period(&elements).max(1);
            let variants = (0..period)
                .map(|cycle| resolve_value_holds(flatten_pattern(&elements, cycle)))
                .collect::<Vec<_>>();
            if variants.iter().any(Vec::is_empty) {
                warn_invalid(ParseKind::Value, pattern);
                Pattern {
                    variants: Vec::new(),
                }
            } else {
                Pattern { variants }
            }
        }
        Err(_) => {
            warn_invalid(ParseKind::Value, pattern);
            Pattern {
                variants: Vec::new(),
            }
        }
    }
}

fn gate_atom(input: &str) -> IResult<&str, PatternElement<bool>> {
    alt((
        map(
            preceded(alt((char('x'), char('X'))), opt(euclidean_args)),
            |euclid| match euclid {
                Some((pulses, steps)) => PatternElement::Group(
                    euclidean_values(pulses, steps, true, false).unwrap_or_default(),
                ),
                None => PatternElement::Atom(true),
            },
        ),
        value(PatternElement::Atom(false), char('.')),
        map(preceded(char('e'), euclidean_args), |(pulses, steps)| {
            PatternElement::Group(euclidean_values(pulses, steps, true, false).unwrap_or_default())
        }),
    ))(input)
}

fn gate_group(input: &str) -> IResult<&str, PatternElement<bool>> {
    map(
        delimited(char('['), gate_sequence, char(']')),
        PatternElement::Group,
    )(input)
}

fn gate_alternate(input: &str) -> IResult<&str, PatternElement<bool>> {
    map(
        delimited(
            char('<'),
            separated_list1(char(';'), gate_sequence),
            char('>'),
        ),
        PatternElement::Alternate,
    )(input)
}

fn gate_item(input: &str) -> IResult<&str, Vec<PatternElement<bool>>> {
    let (input, element) = ws(alt((gate_group, gate_alternate, gate_atom)))(input)?;
    let (input, repeat) = positive_repeat(input)?;
    Ok((input, repeat_elements(element, repeat)))
}

fn gate_sequence(input: &str) -> IResult<&str, Vec<PatternElement<bool>>> {
    map(
        many0(alt((gate_item, value(Vec::new(), ws(char(',')))))),
        |items| items.into_iter().flatten().collect(),
    )(input)
}

fn parse_gates(pattern: &str) -> Pattern<bool> {
    match all_consuming(terminated(gate_sequence, multispace0))(pattern) {
        Ok((_, elements)) if !elements.is_empty() => {
            let pattern = compile_pattern(elements);
            if pattern.is_empty() {
                Pattern {
                    variants: Vec::new(),
                }
            } else {
                pattern
            }
        }
        _ => {
            warn_invalid(ParseKind::Gate, pattern);
            Pattern {
                variants: Vec::new(),
            }
        }
    }
}

#[derive(Clone)]
struct ValuePattern {
    values: Pattern<Sample>,
}

impl ValuePattern {
    fn new(pattern: &str) -> Self {
        Self {
            values: parse_values(pattern),
        }
    }

    fn render(&self, phase: &Frame, cycle_counts: &[usize; CHANNELS]) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.values.is_empty() {
            return frame;
        }
        for (channel, (output, &phase)) in frame.iter_mut().zip(phase).enumerate() {
            let cells = self.values.cells(cycle_counts[channel]);
            *output = cells[cell_index(phase, cells)].value;
        }
        frame
    }
}

#[derive(Clone)]
struct GatePattern {
    gates: Pattern<bool>,
}

impl GatePattern {
    fn new(pattern: &str) -> Self {
        Self {
            gates: parse_gates(pattern),
        }
    }

    fn render_gate(&self, phase: &Frame, cycle_counts: &[usize; CHANNELS]) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.gates.is_empty() {
            return frame;
        }
        for (channel, (output, &phase)) in frame.iter_mut().zip(phase).enumerate() {
            let cells = self.gates.cells(cycle_counts[channel]);
            *output = if cells[cell_index(phase, cells)].value {
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
    previous_phases: [Option<Sample>; CHANNELS],
    cycle_counts: [usize; CHANNELS],
}

impl PatternValue {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: ValuePattern::new(pattern),
            previous_phases: [None; CHANNELS],
            cycle_counts: [0; CHANNELS],
        }
    }
}

impl Op for PatternValue {
    fn perform(&mut self, stack: &mut Stack) {
        let phase = stack.pop();
        update_cycle_counts(&phase, &mut self.previous_phases, &mut self.cycle_counts);
        stack.push(&self.pattern.render(&phase, &self.cycle_counts));
    }
}

pub struct PatternGate {
    pattern: GatePattern,
    previous_phases: [Option<Sample>; CHANNELS],
    cycle_counts: [usize; CHANNELS],
}

impl PatternGate {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: GatePattern::new(pattern),
            previous_phases: [None; CHANNELS],
            cycle_counts: [0; CHANNELS],
        }
    }
}

impl Op for PatternGate {
    fn perform(&mut self, stack: &mut Stack) {
        let phase = stack.pop();
        update_cycle_counts(&phase, &mut self.previous_phases, &mut self.cycle_counts);
        stack.push(&self.pattern.render_gate(&phase, &self.cycle_counts));
    }
}

pub struct PatternTrigger {
    pattern: GatePattern,
    previous_indices: [Option<usize>; CHANNELS],
    previous_phases: [Option<Sample>; CHANNELS],
    cycle_counts: [usize; CHANNELS],
}

impl PatternTrigger {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: GatePattern::new(pattern),
            previous_indices: [None; CHANNELS],
            previous_phases: [None; CHANNELS],
            cycle_counts: [0; CHANNELS],
        }
    }

    fn render(&mut self, phase: &Frame) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.pattern.gates.is_empty() {
            return frame;
        }

        for (channel, (output, &phase)) in frame.iter_mut().zip(phase).enumerate() {
            let phase = wrap_phase(phase);
            let forward_cycle_wrap = self.previous_phases[channel].is_some_and(|prev| prev > phase);
            if forward_cycle_wrap {
                self.cycle_counts[channel] = self.cycle_counts[channel].wrapping_add(1);
            }
            let cells = self.pattern.gates.cells(self.cycle_counts[channel]);
            let index = cell_index(phase, cells);
            let active = cells[index].value;
            let entered_active_cell = self.previous_indices[channel] != Some(index) && active;
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
    previous_phases: [Option<Sample>; CHANNELS],
    cycle_counts: [usize; CHANNELS],
}

impl ClockedPatternValue {
    pub fn new(sample_rate: u32, pattern: &str) -> Self {
        Self {
            cycle: Cycle::new(sample_rate),
            pattern: ValuePattern::new(pattern),
            previous_phases: [None; CHANNELS],
            cycle_counts: [0; CHANNELS],
        }
    }
}

impl Op for ClockedPatternValue {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.cycle.current_then_advance(&cps);
        update_cycle_counts(&phase, &mut self.previous_phases, &mut self.cycle_counts);
        stack.push(&self.pattern.render(&phase, &self.cycle_counts));
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.cycle.phases = other.cycle.phases;
            self.previous_phases = other.previous_phases;
            self.cycle_counts = other.cycle_counts;
        }
    }
}

pub struct ClockedPatternGate {
    cycle: Cycle,
    pattern: GatePattern,
    previous_phases: [Option<Sample>; CHANNELS],
    cycle_counts: [usize; CHANNELS],
}

impl ClockedPatternGate {
    pub fn new(sample_rate: u32, pattern: &str) -> Self {
        Self {
            cycle: Cycle::new(sample_rate),
            pattern: GatePattern::new(pattern),
            previous_phases: [None; CHANNELS],
            cycle_counts: [0; CHANNELS],
        }
    }
}

impl Op for ClockedPatternGate {
    fn perform(&mut self, stack: &mut Stack) {
        let cps = stack.pop();
        let phase = self.cycle.current_then_advance(&cps);
        update_cycle_counts(&phase, &mut self.previous_phases, &mut self.cycle_counts);
        stack.push(&self.pattern.render_gate(&phase, &self.cycle_counts));
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.cycle.phases = other.cycle.phases;
            self.previous_phases = other.previous_phases;
            self.cycle_counts = other.cycle_counts;
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
            self.trigger.cycle_counts = other.trigger.cycle_counts;
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
    fn value_pattern_repeats_atoms_and_groups() {
        let mut pat = PatternValue::new("60*2,[64,67]*2");
        assert_eq!(perform(&mut pat, [0.0, 0.2499]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.4999]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.6249]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.625, 0.7499]), [67.0, 67.0]);
        assert_eq!(perform(&mut pat, [0.75, 0.8749]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.875, 0.9999]), [67.0, 67.0]);
    }

    #[test]
    fn value_pattern_holds_previous_value_for_underscore() {
        let mut pat = PatternValue::new("60,_,[64,_],_");
        assert_eq!(perform(&mut pat, [0.0, 0.2499]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.4999]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.6249]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.625, 0.7499]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.75, 0.9999]), [64.0, 64.0]);
    }

    #[test]
    fn value_pattern_initial_holds_wrap_to_last_value() {
        let mut pat = PatternValue::new("_,60,64,_");
        assert_eq!(perform(&mut pat, [0.0, 0.2499]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.4999]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.9999]), [64.0, 64.0]);
    }

    #[test]
    fn value_pattern_supports_euclidean_rhythms_as_held_values() {
        let mut pat = PatternValue::new("60(3,8)");
        assert_eq!(perform(&mut pat, [0.0, 0.1249]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.125, 0.2499]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.25, 0.3749]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.375, 0.4999]), [60.0, 60.0]);
    }

    #[test]
    fn value_pattern_alternates_each_forward_cycle() {
        let mut pat = PatternValue::new("<60,64;67,72>");
        assert_eq!(perform(&mut pat, [0.0, 0.0]), [60.0, 60.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.5]), [64.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.0, 0.0]), [67.0, 67.0]);
        assert_eq!(perform(&mut pat, [0.5, 0.5]), [72.0, 72.0]);
        assert_eq!(perform(&mut pat, [0.0, 0.0]), [60.0, 60.0]);
    }

    #[test]
    fn value_pattern_alternates_inside_sequences_and_groups() {
        let mut pat = PatternValue::new("60,<64;67>,[72,<76;79>]");
        assert_eq!(perform(&mut pat, [0.0, 0.34]), [60.0, 64.0]);
        assert_eq!(perform(&mut pat, [0.67, 0.84]), [72.0, 76.0]);
        assert_eq!(perform(&mut pat, [0.0, 0.34]), [60.0, 67.0]);
        assert_eq!(perform(&mut pat, [0.67, 0.84]), [72.0, 79.0]);
    }

    #[test]
    fn invalid_value_patterns_output_zero() {
        for pattern in [
            "", "_", "_*4", "60,,64", "abc", "NaN", "inf", "1e309", "60,[64", "60*", "60*0",
            "60(5,4)", "60(3,0)", "<60,64;>",
        ] {
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
    fn gate_pattern_repeats_atoms_and_groups() {
        let mut gate = PatternGate::new("x*2[x.]*2");
        assert_eq!(perform(&mut gate, [0.0, 0.2499]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.25, 0.4999]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.5, 0.6249]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.625, 0.7499]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.75, 0.8749]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.875, 0.9999]), [0.0, 0.0]);
    }

    #[test]
    fn gate_pattern_alternates_each_forward_cycle() {
        let mut gate = PatternGate::new("<x.;.x>");
        assert_eq!(perform(&mut gate, [0.0, 0.0]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.5, 0.5]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.0, 0.0]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.5, 0.5]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.0, 0.0]), [1.0, 1.0]);
    }

    #[test]
    fn gate_pattern_alternates_inside_groups() {
        let mut gate = PatternGate::new("x[<x.;.x>]x");
        assert_eq!(perform(&mut gate, [0.4, 0.55]), [1.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.0, 0.0]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.4, 0.55]), [0.0, 1.0]);
    }

    #[test]
    fn gate_pattern_supports_euclidean_rhythms() {
        let mut gate = PatternGate::new("x(3,8)");
        assert_eq!(perform(&mut gate, [0.0, 0.1249]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.125, 0.2499]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.25, 0.3749]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.375, 0.4999]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.5, 0.6249]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.625, 0.7499]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.75, 0.8749]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.875, 0.9999]), [0.0, 0.0]);
    }

    #[test]
    fn gate_pattern_subdivides_and_repeats_euclidean_rhythms() {
        let mut gate = PatternGate::new("[e(1,2).]*2");
        assert_eq!(perform(&mut gate, [0.0, 0.1249]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.125, 0.4999]), [0.0, 0.0]);
        assert_eq!(perform(&mut gate, [0.5, 0.6249]), [1.0, 1.0]);
        assert_eq!(perform(&mut gate, [0.625, 0.9999]), [0.0, 0.0]);
    }

    #[test]
    fn invalid_gate_patterns_output_zero() {
        for pattern in [
            "", "x..q", "1..0", "x[.", "x*", "x*0", "e(5,4)", "e(3,0)", "e(3,)",
        ] {
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
