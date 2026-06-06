use ahash::RandomState;
use audio_ops::*;
use audio_vm::{AtomicFrame, AtomicSample, Op, Program, Sample, Statement};
#[cfg(test)]
use audio_vm::Frame;
use rand::{rngs::SmallRng, seq::SliceRandom};
use regex::Regex;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fs::File;
use std::sync::{Arc, atomic::Ordering};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const HELP: &str = include_str!("help.adoc");
pub const PARAMETERS: usize = 16;

pub struct Context {
    pub input: Arc<AtomicFrame>,
    pub params: [Arc<AtomicSample>; PARAMETERS],
    pub tables: HashMap<String, Arc<Vec<AtomicFrame>>, RandomState>,
    pub variables: HashMap<String, Arc<AtomicFrame>, RandomState>,
}

impl Context {
    pub fn new() -> Self {
        Context {
            input: Default::default(),
            params: Default::default(),
            tables: HashMap::with_hasher(RandomState::new()),
            variables: HashMap::with_hasher(RandomState::new()),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Context::new()
    }
}

#[derive(
    Archive,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    RkyvSerialize,
    RkyvDeserialize,
    Serialize,
    Deserialize,
)]
pub struct TextOp {
    pub id: u64,
    pub op: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waveform {
    Cosine,
    CosineFast,
    Sine,
    SineFast,
    Triangle,
}

impl Waveform {
    fn from_oscillator(op: &str) -> Option<Self> {
        match op {
            "c" => Some(Self::Cosine),
            "c'" => Some(Self::CosineFast),
            "s" => Some(Self::Sine),
            "s'" => Some(Self::SineFast),
            "t" => Some(Self::Triangle),
            _ => None,
        }
    }

    fn function(self) -> fn(Sample) -> Sample {
        match self {
            Self::Cosine => pure::cosine,
            Self::CosineFast => pure::cosine_fast,
            Self::Sine => pure::sine,
            Self::SineFast => pure::sine_fast,
            Self::Triangle => pure::triangle,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum OptimizedOp {
    Text(TextOp),
    Constant { id: u64, value: Sample },
    FixedOsc { id: u64, waveform: Waveform, frequency: Sample },
    AddConst { id: u64, value: Sample },
    MulConst { id: u64, value: Sample },
    SubConst { id: u64, value: Sample },
    RSubConst { id: u64, value: Sample },
    DivConst { id: u64, value: Sample },
    RDivConst { id: u64, value: Sample },
}

fn load_table(path: &str) -> Option<Vec<AtomicFrame>> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = std::path::Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref()?.audio()?,
            &AudioDecoderOptions::default(),
        )
        .ok()?;
    let mut samples = Vec::<Sample>::new();
    let mut table = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => break,
            Err(_) => continue,
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = decoded.spec();
        samples.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut samples);
        let channels = spec.channels().count();
        for samples in samples.chunks(channels) {
            let mut frame: AtomicFrame = Default::default();
            for (a, &x) in frame.iter_mut().zip(samples) {
                a.store(x.to_bits(), Ordering::Relaxed);
            }
            table.push(frame);
        }
    }

    Some(table)
}

pub fn compile_program(ops: &[TextOp], sample_rate: u32, ctx: &mut Context) -> Program {
    let ops = optimize_terms(&rewrite_terms(ops));
    let mut program = SmallVec::new();
    macro_rules! push {
        ( $id:ident, $class:ident ) => {
            program.push(Statement {
                id: $id,
                op: Box::new($class::new()) as Box<dyn Op>,
            })
        };
    }
    macro_rules! push_args {
        ( $id:ident, $class:ident, $($rest:tt)* ) => {
            program.push(Statement { id: $id, op: Box::new($class::new($($rest)*)) as Box<dyn Op> })
        };
    }
    for optimized_op in ops {
        let TextOp { id, op } = match optimized_op {
            OptimizedOp::Text(text_op) => text_op,
            OptimizedOp::Constant { id, value } => {
                push_args!(id, Constant, value);
                continue;
            }
            OptimizedOp::FixedOsc {
                id,
                waveform,
                frequency,
            } => {
                push_args!(id, FixedOsc, sample_rate, frequency, waveform.function());
                continue;
            }
            OptimizedOp::AddConst { id, value } => {
                push_args!(id, AddConst, value);
                continue;
            }
            OptimizedOp::MulConst { id, value } => {
                push_args!(id, MulConst, value);
                continue;
            }
            OptimizedOp::SubConst { id, value } => {
                push_args!(id, SubConst, value);
                continue;
            }
            OptimizedOp::RSubConst { id, value } => {
                push_args!(id, RSubConst, value);
                continue;
            }
            OptimizedOp::DivConst { id, value } => {
                push_args!(id, DivConst, value);
                continue;
            }
            OptimizedOp::RDivConst { id, value } => {
                push_args!(id, RDivConst, value);
                continue;
            }
        };

        match op.as_str() {
            "*" | "mul" => push_args!(id, Fn2, pure::mul),
            "+" | "add" => push_args!(id, Fn2, pure::add),
            "-" | "sub" => push_args!(id, Fn2, pure::sub),
            "/" | "div" => push_args!(id, Fn2, pure::safe_div),
            "\\" => push_args!(id, Fn1, pure::safe_recip),
            "^" | "pow" => push_args!(id, Fn2, pure::pow),
            "%" | "mod" => push_args!(id, Fn2, pure::modulo),
            "adsr" => push_args!(id, ADSR, sample_rate),
            "amp2db" | "a2db" => push_args!(id, Fn1, pure::amp2db),
            "c" => push_args!(id, Osc, sample_rate, pure::cosine),
            "c'" => push_args!(id, Osc, sample_rate, pure::cosine_fast),
            "cheb2" => push_args!(id, Fn1, pure::cheb2),
            "cheb3" => push_args!(id, Fn1, pure::cheb3),
            "cheb4" => push_args!(id, Fn1, pure::cheb4),
            "cheb5" => push_args!(id, Fn1, pure::cheb5),
            "cheb6" => push_args!(id, Fn1, pure::cheb6),
            "circle" => push_args!(id, Fn1, pure::circle),
            "clamp" => push_args!(id, Fn3, pure::clamp),
            "clip" => push_args!(id, Fn1, pure::clip),
            "cos" => push_args!(id, Fn1, pure::cos),
            "cos'" => push_args!(id, Fn1, pure::cos_fast),
            "cosh" => push_args!(id, Fn1, pure::cosh),
            "cosine" => push_args!(id, OscPhase, sample_rate, pure::cosine),
            "cosine'" => push_args!(id, OscPhase, sample_rate, pure::cosine_fast),
            "db2amp" | "db2a" => push_args!(id, Fn1, pure::db2amp),
            "dm" | "dmetro" => push_args!(id, DMetro, sample_rate),
            "dmh" | "dmetro_hold" => push_args!(id, DMetroHold, sample_rate),
            "dup" => push!(id, Dup),
            "exp" => push_args!(id, Fn1, pure::exp),
            "f2m" | "freq2midi" => push_args!(id, Fn1, pure::freq2midi),
            "h" | "bqhpf" => push_args!(id, BiQuad, sample_rate, make_hpf_coefficients),
            "hpf" => push_args!(id, HPF, sample_rate),
            "impulse" => push_args!(id, Impulse, sample_rate),
            "in" | "input" => push_args!(id, Input, Arc::clone(&ctx.input)),
            "l" | "bqlpf" => push_args!(id, BiQuad, sample_rate, make_lpf_coefficients),
            "linlin" | "project" => push_args!(id, Fn5, pure::linlin),
            "lpf" => push_args!(id, LPF, sample_rate),
            "m" | "metro" => push_args!(id, Metro, sample_rate),
            "m2f" | "midi2freq" | "#" => push_args!(id, Fn1, pure::midi2freq),
            "max" => push_args!(id, Fn2, pure::max),
            "mh" | "metro_hold" => push_args!(id, MetroHold, sample_rate),
            "min" => push_args!(id, Fn2, pure::min),
            "n" | "noise" | "whiteNoise" => push!(id, WhiteNoise),
            "oneshot" | "shot" => push_args!(id, OneShot, sample_rate),
            "p" => push_args!(id, Pulse, sample_rate),
            "pan1" => push!(id, Pan1),
            "pan2" => push!(id, Pan2),
            "panx" => push!(id, Pan3),
            "pi" => push_args!(id, Constant, std::f64::consts::PI),
            "tau" => push_args!(id, Constant, 2.0 * std::f64::consts::PI),
            "pitch" => push_args!(id, Yin, sample_rate, 1024, 64, 0.2),
            "pop" => push!(id, Pop),
            "prime" => push!(id, Prime),
            "pulse" => push_args!(id, PulsePhase, sample_rate),
            "q" | "quantize" => push_args!(id, Fn2, pure::quantize),
            "r" | "range" => push_args!(id, Fn3, pure::range),
            "rot" => push!(id, Rot),
            "round" => push_args!(id, Fn1, pure::round),
            "s" => push_args!(id, Osc, sample_rate, pure::sine),
            "s'" => push_args!(id, Osc, sample_rate, pure::sine_fast),
            "saw" => push_args!(id, Phasor0, sample_rate),
            "sh" | "sample&hold" => push!(id, SampleAndHold),
            "ssh" => push!(id, SmoothSampleAndHold),
            "silence" => push_args!(id, Constant, 0.0),
            "sin" => push_args!(id, Fn1, pure::sin),
            "sin'" => push_args!(id, Fn1, pure::sin_fast),
            "sinc" => push_args!(id, Fn1, pure::sinc),
            "sinc'" => push_args!(id, Fn1, pure::sinc_fast),
            "sine" => push_args!(id, OscPhase, sample_rate, pure::sine),
            "sine'" => push_args!(id, OscPhase, sample_rate, pure::sine_fast),
            "sinh" => push_args!(id, Fn1, pure::sinh),
            "spectral_shuffle" => {
                let mut rng = Box::new(rand::make_rng::<SmallRng>());
                push_args!(
                    id,
                    SpectralTransform,
                    2048, // window_size
                    64,   // period
                    Box::new(move |freqs| freqs.shuffle(&mut rng)),
                )
            }
            "st1" => {
                push_args!(
                    id,
                    SpectralTransform,
                    2048, // window_size
                    64,   // period
                    Box::new(|freqs| {
                        let mut max = 0.0;
                        let mut max_idx = 0;
                        for (i, freq) in freqs.iter().enumerate() {
                            if freq.re > max {
                                max = freq.re;
                                max_idx = i;
                            }
                        }
                        for (i, freq) in freqs.iter_mut().enumerate() {
                            if i != max_idx {
                                *freq = Default::default();
                            }
                        }
                    }),
                )
            }
            "spectral_reverse" => {
                push_args!(
                    id,
                    SpectralTransform,
                    2048, // window_size
                    64,   // period
                    Box::new(|freqs| freqs.reverse()),
                )
            }
            "sr" => push_args!(id, Constant, sample_rate as _),
            "swap" => push!(id, Swap),
            "t" => push_args!(id, Osc, sample_rate, pure::triangle),
            "tan" => push_args!(id, Fn1, pure::tan),
            "tan'" => push_args!(id, Fn1, pure::tan_fast),
            "tanh" => push_args!(id, Fn1, pure::tanh),
            "tri" => push_args!(id, OscPhase, sample_rate, pure::triangle),
            "tline" => push_args!(id, Transition, sample_rate, pure::linear_curve),
            "tquad" => push_args!(id, Transition, sample_rate, pure::quadratic_curve),
            "unit" => push_args!(id, Fn1, pure::unit),
            "w" => push_args!(id, Phasor, sample_rate),
            "wah" => push_args!(id, WahPedal, sample_rate),
            "wrap" => push_args!(id, Fn1, pure::wrap),
            _ => match op.parse::<Sample>() {
                Ok(x) => push_args!(id, Constant, x),
                Err(_) => {
                    let tokens = op.split(':').collect::<Vec<_>>();
                    match tokens[0] {
                        "" | "dig" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(n) => push_args!(id, Dig, n),
                                Err(_) => {
                                    log::warn!("Can't parse {} as depth", x);
                                }
                            },
                            None => {
                                log::warn!("Missing depth parameter.");
                            }
                        },
                        "-" | "bury" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(n) => push_args!(id, Bury, n),
                                Err(_) => {
                                    log::warn!("Can't parse {} as depth", x);
                                }
                            },
                            None => {
                                log::warn!("Missing depth parameter.");
                            }
                        },
                        "ch" | "channel" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(n) => push_args!(id, Channel, n),
                                Err(_) => {
                                    log::warn!("Can't parse {} as channel number", x);
                                }
                            },
                            None => {
                                log::warn!("Missing channel number parameter.");
                            }
                        },
                        "dl" | "delay" => match tokens.get(1) {
                            Some(x) => {
                                push_args!(id, Delay, sample_rate, x.parse::<f64>().unwrap_or(60.0))
                            }
                            None => push_args!(id, Delay, sample_rate, 60.0),
                        },
                        "fb" | "feedback" => match tokens.get(1) {
                            Some(x) => push_args!(
                                id,
                                Feedback,
                                sample_rate,
                                x.parse::<f64>().unwrap_or(60.0)
                            ),
                            None => push_args!(id, Feedback, sample_rate, 60.0),
                        },
                        "get" => match tokens
                            .get(1)
                            .map(|x| ctx.variables.entry(x.to_string()).or_default())
                        {
                            Some(var) => {
                                push_args!(id, ReadVariable, Arc::clone(var));
                            }
                            None => {
                                log::warn!("Missing var name parameter.");
                            }
                        },
                        "set" => match tokens
                            .get(1)
                            .map(|x| ctx.variables.entry(x.to_string()).or_default())
                        {
                            Some(var) => {
                                push_args!(id, WriteVariable, Arc::clone(var));
                            }
                            None => {
                                log::warn!("Missing var name parameter.");
                            }
                        },
                        "var" => match tokens
                            .get(1)
                            .map(|x| ctx.variables.entry(x.to_string()).or_default())
                        {
                            Some(var) => {
                                push_args!(id, TakeVariable, Arc::clone(var));
                            }
                            None => {
                                log::warn!("Missing var name parameter.");
                            }
                        },
                        "ft" | "ftab" | "filetable" => match tokens.get(1) {
                            Some(path) => {
                                if !ctx.tables.contains_key(*path)
                                    && let Some(table) = load_table(path) {
                                        let table = Arc::new(table);
                                        ctx.tables.insert(path.to_string(), table);
                                    }
                                if let Some(table) = ctx.tables.get(*path) {
                                    push_args!(id, TableReader, sample_rate, Arc::clone(table));
                                }
                            }
                            None => {
                                log::warn!("Missing table file parameter.");
                            }
                        },
                        "rt" | "rtab" | "readtable" => {
                            match tokens.get(1).and_then(|x| ctx.tables.get(*x)) {
                                Some(table) => {
                                    push_args!(id, TableReader, sample_rate, Arc::clone(table));
                                }
                                None => {
                                    log::warn!("Missing table name parameter.");
                                }
                            }
                        }
                        "wt" | "wtab" | "writetable" => match tokens.get(2) {
                            Some(x) => match x.parse::<Sample>() {
                                Ok(size) => {
                                    let len = (size * (sample_rate as Sample)) as usize;
                                    let mut table = Vec::with_capacity(len);
                                    for _ in 0..len {
                                        table.push(Default::default());
                                    }
                                    let table = Arc::new(table);
                                    let table_name = String::from(tokens[1]);
                                    ctx.tables.insert(table_name, Arc::clone(&table));
                                    push_args!(id, TableWriter, table);
                                }
                                Err(_) => {
                                    log::warn!("Can't parse {} as table length.", x);
                                }
                            },
                            None => {
                                log::warn!("Missing table name or length parameter.");
                            }
                        },
                        "conv" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(window_size) => push_args!(id, Convolution, window_size),
                                Err(_) => {
                                    log::warn!("Can't parse {} as kernel length.", x);
                                }
                            },
                            None => {
                                log::warn!("Missing kernel length parameter.");
                            }
                        },
                        "convm" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(window_size) => push_args!(id, ConvolutionM, window_size),
                                Err(_) => {
                                    log::warn!("Can't parse {} as kernel length.", x);
                                }
                            },
                            None => {
                                log::warn!("Missing kernel length parameter.");
                            }
                        },
                        "param" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(n) => push_args!(id, Param, Arc::clone(&ctx.params[n])),
                                Err(_) => {
                                    log::warn!("Can't parse {} as param number", x);
                                }
                            },
                            None => {
                                log::warn!("Missing param number parameter.");
                            }
                        },
                        "norm" => match tokens.get(1) {
                            Some(x) => push_args!(id, Normalise, x.parse::<usize>().unwrap_or(256)),
                            None => push_args!(id, Normalise, 256),
                        },
                        _ => {
                            log::warn!("Unknown token: {}", op);
                        }
                    }
                }
            },
        }
    }
    program
}

pub fn get_help() -> HashMap<String, String> {
    let mut result = HashMap::new();
    for item in Regex::new(r"(?P<term>(\w+(:<\w+>)*(, )*)+)::(?P<definition>.+)")
        .unwrap()
        .captures_iter(HELP)
    {
        let definition = item.name("definition").unwrap().as_str().trim();
        for term in item.name("term").unwrap().as_str().split(", ") {
            result.insert(
                term.split(':').next().unwrap().to_owned(),
                definition.to_owned(),
            );
        }
    }
    result
}

pub fn get_op_groups() -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    let group_re = Regex::new("=== (.+)").unwrap();
    let item_re = Regex::new(r"(?P<term>(\w+(:<\w+>)*(, )*)+)::").unwrap();
    let mut current_group = None;
    for line in HELP.split('\n') {
        if let Some(m) = group_re.captures(line) {
            if let Some(group) = current_group {
                result.push(group);
            }
            current_group = Some((m.get(1).unwrap().as_str().to_owned(), Vec::new()));
        } else if let Some(m) = item_re.captures(line)
            && let Some(group) = &mut current_group {
                group.1.extend(
                    m.name("term")
                        .unwrap()
                        .as_str()
                        .split(", ")
                        .map(|x| x.to_owned()),
                );
            }
    }
    result
}

fn rewrite_terms(stmts: &[TextOp]) -> Vec<TextOp> {
    let mut result: Vec<TextOp> = Vec::new();
    let mut new_term: Option<Term> = None;
    let mut terms: HashMap<String, Term> = Default::default();
    let mut stack: Vec<TextOp> = Vec::from(stmts);
    stack.reverse();
    while let Some(stmt) = stack.pop() {
        // This is a known term, let's rewrite it...
        if let Some(term) = terms.get(&stmt.op) {
            // ...but not when we are defining a new term.
            if let Some(term) = new_term.as_mut() {
                term.ops.push(stmt);
            } else if term.holes <= result.len() {
                // Steal ops from the output to fill the holes.
                let mut holes = result.drain((result.len() - term.holes)..);
                // Not pushing rewrited terms directly onto the stack
                // as we need to reverse them.
                let mut rewrite = Vec::new();
                for t in &term.ops {
                    // Hole filling already has its own unique id,
                    // no need to change it...
                    rewrite.push(if t.op.contains("?") {
                        holes.next().unwrap()
                    // ...but term literals have to be salted,
                    // as they are copied every time term is encountered.
                    } else {
                        let mut t = t.clone();
                        t.id = t.id.overflowing_add(stmt.id).0;
                        t
                    });
                }
                // Push rewrites onto the stack, not result,
                // to have them processed (as may contain further terms).
                for op in rewrite.drain(..).rev() {
                    stack.push(op);
                }
            }
        } else if stmt.op.starts_with("[") {
            new_term = Some(Term {
                holes: 0,
                ops: Vec::new(),
            });
            let token: String = stmt.op.chars().skip(1).collect();
            if !token.is_empty() {
                stack.push(TextOp {
                    id: stmt.id,
                    op: token,
                });
            }
        } else if stmt.op == "]" {
            if let Some(term) = new_term.take()
                && let Some(op) = stack.pop() {
                    terms.insert(op.op, term);
                }
        } else if stmt.op.ends_with("]") {
            if new_term.is_some() {
                stack.push(TextOp {
                    id: 0,
                    op: "]".to_string(),
                });
                stack.push(TextOp {
                    id: stmt.id,
                    op: stmt.op.chars().take(stmt.op.len() - 1).collect(),
                });
            }
        } else {
            if let Some(term) = new_term.as_mut() {
                if stmt.op.contains("?") {
                    term.holes += 1;
                }
                term.ops.push(stmt);
            } else {
                result.push(stmt);
            }
        }
    }
    result
}

fn optimize_terms(stmts: &[TextOp]) -> Vec<OptimizedOp> {
    let mut result = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        result.push(optimized_op(stmt));

        loop {
            if let Some(folded) = fold_tail_constants(&result) {
                result.truncate(result.len() - 3);
                result.push(folded);
            } else if let Some(folded) = fold_tail_fixed_osc_terms(&result) {
                result.truncate(result.len() - 2);
                result.push(folded);
            } else if let Some(folded) = fold_tail_binary_const_terms(&result) {
                result.truncate(result.len() - 3);
                result.extend(folded);
            } else {
                break;
            }
        }
    }

    result
}

fn optimized_op(stmt: &TextOp) -> OptimizedOp {
    match stmt.op.parse::<Sample>() {
        Ok(value) => OptimizedOp::Constant { id: stmt.id, value },
        Err(_) => OptimizedOp::Text(stmt.clone()),
    }
}

fn fold_tail_binary_const_terms(stmts: &[OptimizedOp]) -> Option<[OptimizedOp; 2]> {
    let [rest @ .., a, b, op] = stmts else {
        return None;
    };
    let _ = rest;

    let OptimizedOp::Text(op) = op else {
        return None;
    };

    match (op.op.as_str(), const_value(a), const_value(b)) {
        ("+" | "add", _, Some(value)) => Some([a.clone(), OptimizedOp::AddConst { id: op.id, value }]),
        ("+" | "add", Some(value), _) => Some([b.clone(), OptimizedOp::AddConst { id: op.id, value }]),
        ("*" | "mul", _, Some(value)) => Some([a.clone(), OptimizedOp::MulConst { id: op.id, value }]),
        ("*" | "mul", Some(value), _) => Some([b.clone(), OptimizedOp::MulConst { id: op.id, value }]),
        ("-" | "sub", _, Some(value)) => Some([a.clone(), OptimizedOp::SubConst { id: op.id, value }]),
        ("-" | "sub", Some(value), _) => Some([b.clone(), OptimizedOp::RSubConst { id: op.id, value }]),
        ("/" | "div", _, Some(value)) => Some([a.clone(), OptimizedOp::DivConst { id: op.id, value }]),
        ("/" | "div", Some(value), _) => Some([b.clone(), OptimizedOp::RDivConst { id: op.id, value }]),
        _ => None,
    }
}

fn fold_tail_fixed_osc_terms(stmts: &[OptimizedOp]) -> Option<OptimizedOp> {
    let [rest @ .., frequency, oscillator] = stmts else {
        return None;
    };
    let _ = rest;

    let frequency = const_value(frequency)?;
    let OptimizedOp::Text(oscillator) = oscillator else {
        return None;
    };
    let waveform = Waveform::from_oscillator(&oscillator.op)?;

    Some(OptimizedOp::FixedOsc {
        id: oscillator.id,
        waveform,
        frequency,
    })
}

fn fold_tail_constants(stmts: &[OptimizedOp]) -> Option<OptimizedOp> {
    let [rest @ .., a, b, op] = stmts else {
        return None;
    };
    let _ = rest;

    let a = const_value(a)?;
    let b = const_value(b)?;
    let OptimizedOp::Text(op) = op else {
        return None;
    };

    let value = match op.op.as_str() {
        "+" | "add" => a + b,
        "*" | "mul" => a * b,
        "-" | "sub" => a - b,
        "/" | "div" => {
            if b != 0.0 {
                a / b
            } else {
                0.0
            }
        }
        "^" | "pow" => a.powf(b),
        "%" | "mod" => a % b,
        "q" | "quantize" => {
            if b != 0.0 {
                (a / b).round() * b
            } else {
                0.0
            }
        }
        _ => return None,
    };

    if value.is_finite() {
        Some(OptimizedOp::Constant { id: op.id, value })
    } else {
        None
    }
}

fn const_value(op: &OptimizedOp) -> Option<Sample> {
    match op {
        OptimizedOp::Constant { value, .. } => Some(*value),
        _ => None,
    }
}

struct Term {
    holes: usize,
    ops: Vec<TextOp>,
}

#[cfg(test)]
mod tests {
    use super::Context;
    use super::*;

    #[test]
    fn rewrite_terms_does_its_thing() {
        assert_eq!(
            rewrite_terms(&[TextOp {
                    id: 1,
                    op: "[?".to_string()
                },
                TextOp {
                    id: 10,
                    op: "s]".to_string()
                },
                TextOp {
                    id: 100,
                    op: "foo".to_string()
                },
                TextOp {
                    id: 1000,
                    op: "[?".to_string()
                },
                TextOp {
                    id: 10000,
                    op: "foo".to_string()
                },
                TextOp {
                    id: 100000,
                    op: "+]".to_string()
                },
                TextOp {
                    id: 1000000,
                    op: "bar".to_string()
                },
                TextOp {
                    id: 10000000,
                    op: "1".to_string()
                },
                TextOp {
                    id: 100000000,
                    op: "bar".to_string()
                },
                TextOp {
                    id: 1000000000,
                    op: "2".to_string()
                },
                TextOp {
                    id: 10000000000,
                    op: "bar".to_string()
                }]),
            vec![
                TextOp {
                    id: 10000000,
                    op: "1".to_string()
                },
                TextOp {
                    id: 100010010,
                    op: "s".to_string()
                },
                TextOp {
                    id: 100100000,
                    op: "+".to_string()
                },
                TextOp {
                    id: 1000000000,
                    op: "2".to_string()
                },
                TextOp {
                    id: 10000010010,
                    op: "s".to_string()
                },
                TextOp {
                    id: 10000100000,
                    op: "+".to_string()
                },
            ]
        );
    }

    fn op(id: u64, op: &str) -> TextOp {
        TextOp {
            id,
            op: op.to_owned(),
        }
    }

    fn text(id: u64, token: &str) -> OptimizedOp {
        OptimizedOp::Text(op(id, token))
    }

    fn constant(id: u64, value: Sample) -> OptimizedOp {
        OptimizedOp::Constant { id, value }
    }

    #[test]
    fn optimize_terms_folds_constant_arithmetic() {
        assert_eq!(
            optimize_terms(&[
                op(1, "2"),
                op(2, "3"),
                op(3, "+"),
                op(4, "4"),
                op(5, "*"),
            ]),
            vec![constant(5, 20.0)]
        );
    }

    #[test]
    fn optimize_terms_leaves_dynamic_stack_arithmetic_alone() {
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "param:1"), op(3, "+")]),
            vec![text(1, "input"), text(2, "param:1"), text(3, "+")]
        );
    }

    #[test]
    fn optimize_terms_specializes_fixed_frequency_oscillators() {
        assert_eq!(
            optimize_terms(&[op(1, "440"), op(2, "s'")]),
            vec![OptimizedOp::FixedOsc {
                id: 2,
                waveform: Waveform::SineFast,
                frequency: 440.0,
            }]
        );
    }

    #[test]
    fn optimize_terms_specializes_binary_ops_with_constants() {
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "0.5"), op(3, "*")]),
            vec![text(1, "input"), OptimizedOp::MulConst { id: 3, value: 0.5 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "0.5"), op(2, "input"), op(3, "*")]),
            vec![text(2, "input"), OptimizedOp::MulConst { id: 3, value: 0.5 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "+")]),
            vec![text(1, "input"), OptimizedOp::AddConst { id: 3, value: 2.0 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "-")]),
            vec![text(1, "input"), OptimizedOp::SubConst { id: 3, value: 2.0 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "2"), op(2, "input"), op(3, "-")]),
            vec![text(2, "input"), OptimizedOp::RSubConst { id: 3, value: 2.0 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "/")]),
            vec![text(1, "input"), OptimizedOp::DivConst { id: 3, value: 2.0 }]
        );
        assert_eq!(
            optimize_terms(&[op(1, "2"), op(2, "input"), op(3, "/")]),
            vec![text(2, "input"), OptimizedOp::RDivConst { id: 3, value: 2.0 }]
        );
    }

    fn run_once(ops: &[TextOp], context: &mut Context) -> Frame {
        let mut vm = audio_vm::VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(compile_program(ops, 100, context));
        vm.play();
        vm.next_frame()
    }

    #[test]
    fn compile_program_evaluates_stack_arithmetic_and_aliases() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[
                    op(1, "2"),
                    op(2, "3"),
                    op(3, "add"),
                    op(4, "4"),
                    op(5, "mul")
                ],
                &mut context
            ),
            [20.0, 20.0]
        );
    }

    #[test]
    fn compile_program_reads_input_and_parameters_from_shared_context() {
        let mut context = Context::new();
        context.input[0].store(0.25f64.to_bits(), Ordering::Relaxed);
        context.input[1].store((-0.5f64).to_bits(), Ordering::Relaxed);
        context.params[2].store(4.0f64.to_bits(), Ordering::Relaxed);

        assert_eq!(
            run_once(
                &[op(1, "input"), op(2, "param:2"), op(3, "*")],
                &mut context
            ),
            [1.0, -2.0]
        );
    }

    #[test]
    fn compile_program_shares_named_variables_between_ops() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[
                    op(1, "7"),
                    op(2, "set:answer"),
                    op(3, "get:answer"),
                    op(4, "+")
                ],
                &mut context
            ),
            [14.0, 14.0]
        );
        assert!(context.variables.contains_key("answer"));
    }

    #[test]
    fn compile_program_creates_and_writes_named_tables() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[op(1, "0.75"), op(2, "1"), op(3, "wt:loop:0.01")],
                &mut context
            ),
            [0.75, 0.75]
        );

        let table = context.tables.get("loop").expect("created table");
        assert_eq!(table.len(), 1);
        assert_eq!(
            [
                f64::from_bits(table[0][0].load(Ordering::Relaxed)),
                f64::from_bits(table[0][1].load(Ordering::Relaxed)),
            ],
            [0.75, 0.75]
        );
    }

    #[test]
    fn help_index_contains_aliases_and_grouped_terms() {
        let help = get_help();
        assert_eq!(
            help.get("+").map(String::as_str),
            help.get("add").map(String::as_str)
        );
        assert!(help.contains_key("metro"));

        let groups = get_op_groups();
        assert!(groups.iter().any(|(group, terms)| group == "Triggers"
            && terms.iter().any(|term| term.starts_with("metro"))));
    }
}
