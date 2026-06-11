use ahash::RandomState;
use audio_ops::*;
#[cfg(test)]
use audio_vm::Frame;
use audio_vm::{AtomicFrame, AtomicSample, Op, Program, Sample, Statement};
use rand::{rngs::SmallRng, seq::SliceRandom};
use regex::Regex;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::sync::{Arc, atomic::Ordering};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
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
    Constant {
        id: u64,
        value: Sample,
    },
    FixedOsc {
        id: u64,
        waveform: Waveform,
        frequency: Sample,
    },
    AddConst {
        id: u64,
        value: Sample,
    },
    MulConst {
        id: u64,
        value: Sample,
    },
    SubConst {
        id: u64,
        value: Sample,
    },
    DivConst {
        id: u64,
        value: Sample,
    },
}

fn load_table(path: &str) -> Option<Vec<AtomicFrame>> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
    {
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
            Ok(None) | Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => {
                break;
            }
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

/// Sentinel ops produced by `rewrite_terms` to delimit a quotation (a bracket
/// group consumed by a quotation-taking op such as `poly`) in the flat op
/// stream. Performers never type these; nested quotations nest markers.
// Note: these must not start with `[`, end with `]`, or contain `:` so they
// can never collide with the bracket/template branches in `rewrite_terms`.
const QUOTE_OPEN: &str = "\u{1}(";
const QUOTE_CLOSE: &str = "\u{1})";

fn is_quotation_consumer(op: &str) -> bool {
    op == "poly" || op.starts_with("poly:")
}

pub fn compile_program(ops: &[TextOp], sample_rate: u32, ctx: &mut Context) -> Program {
    let ops = rewrite_terms(ops);
    let mut program = Vec::new();
    compile_ops(&ops, sample_rate, ctx, &mut program);
    program
}

/// Compile an op stream: quotations (`QUOTE_OPEN .. QUOTE_CLOSE` followed by
/// a quotation consumer) become container ops, everything between them is
/// compiled as plain segments. Returns true if compilation was stopped by
/// `return`.
fn compile_ops(ops: &[TextOp], sample_rate: u32, ctx: &mut Context, program: &mut Program) -> bool {
    let mut segment_start = 0;
    let mut i = 0;
    while i < ops.len() {
        if ops[i].op != QUOTE_OPEN {
            i += 1;
            continue;
        }
        if compile_segment(&ops[segment_start..i], sample_rate, ctx, program) {
            return true;
        }
        // Find the matching close marker, allowing nested quotations.
        let mut depth = 0usize;
        let mut close = None;
        for (j, op) in ops.iter().enumerate().skip(i + 1) {
            if op.op == QUOTE_OPEN {
                depth += 1;
            } else if op.op == QUOTE_CLOSE {
                if depth == 0 {
                    close = Some(j);
                    break;
                }
                depth -= 1;
            }
        }
        let Some(close) = close else {
            // Unbalanced markers should not happen; skip forgivingly.
            log::warn!("Unbalanced quotation; ignoring it.");
            segment_start = i + 1;
            i += 1;
            continue;
        };
        let body = &ops[i + 1..close];
        match ops.get(close + 1) {
            Some(consumer) if is_quotation_consumer(&consumer.op) => {
                program.push(Statement {
                    id: consumer.id,
                    op: Box::new(compile_poly(&consumer.op, body, sample_rate, ctx)),
                });
                i = close + 2;
            }
            _ => {
                log::warn!("Quotation is not followed by a quotation consumer; ignoring it.");
                i = close + 1;
            }
        }
        segment_start = i;
    }
    compile_segment(&ops[segment_start..], sample_rate, ctx, program)
}

/// Compile `<quotation> poly:N`: N voices, each an independently compiled
/// instance of the body sharing node ids (state migrates by voice index +
/// node id). Invalid argument or empty body compiles to a forgiving
/// zero-voice op which preserves stack shape.
fn compile_poly(op: &str, body: &[TextOp], sample_rate: u32, ctx: &mut Context) -> Poly {
    let Some(voices) = op
        .split(':')
        .nth(1)
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|&n| n > 0)
    else {
        log::warn!(
            "Can't parse voice count in {}; compiling to a zero-voice poly.",
            op
        );
        return Poly::empty();
    };
    let bodies: Vec<_> = (0..voices)
        .map(|_| {
            let mut voice = Vec::new();
            compile_ops(body, sample_rate, ctx, &mut voice);
            voice.into_boxed_slice()
        })
        .collect();
    if bodies.first().is_none_or(|body| body.is_empty()) {
        log::warn!("Empty poly voice body; compiling to a zero-voice poly.");
        return Poly::empty();
    }
    Poly::new(bodies)
}

fn compile_segment(
    ops: &[TextOp],
    sample_rate: u32,
    ctx: &mut Context,
    program: &mut Program,
) -> bool {
    let ops = optimize_terms(ops);
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
            OptimizedOp::DivConst { id, value } => {
                push_args!(id, DivConst, value);
                continue;
            }
        };

        if op.trim().is_empty() {
            continue;
        }

        if let Some(name) = op.strip_prefix('<').filter(|name| !name.is_empty()) {
            let var = ctx.variables.entry(name.to_string()).or_default();
            push_args!(id, ReadVariable, Arc::clone(var));
            continue;
        }

        if let Some(name) = op.strip_prefix('=').filter(|name| !name.is_empty()) {
            let var = ctx.variables.entry(name.to_string()).or_default();
            push_args!(id, WriteVariable, Arc::clone(var));
            continue;
        }

        if let Some(name) = op.strip_prefix('>').filter(|name| !name.is_empty()) {
            let var = ctx.variables.entry(name.to_string()).or_default();
            push_args!(id, TakeVariable, Arc::clone(var));
            continue;
        }

        match op.as_str() {
            "return" | "ret" | "!" => return true,
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
            "cycle" | "cy" => push_args!(id, Cycle, sample_rate),
            "db2amp" | "db2a" => push_args!(id, Fn1, pure::db2amp),
            "dm" | "dmetro" => push_args!(id, DMetro, sample_rate),
            "dmh" | "dmetro_hold" => push_args!(id, DMetroHold, sample_rate),
            "dup" => push!(id, Dup),
            "exp" => push_args!(id, Fn1, pure::exp),
            "biexp" => push_args!(id, Fn3, pure::biexp),
            "expexp" => push_args!(id, Fn5, pure::expexp),
            "explin" => push_args!(id, Fn5, pure::explin),
            "f2m" | "freq2midi" => push_args!(id, Fn1, pure::freq2midi),
            "bp" | "bqbpf" => push_args!(id, BiQuad, sample_rate, make_bpf_coefficients),
            "h" | "bqhpf" => push_args!(id, BiQuad, sample_rate, make_hpf_coefficients),
            "hpf" => push_args!(id, HPF, sample_rate),
            "impulse" => push_args!(id, Impulse, sample_rate),
            "in" | "input" => push_args!(id, Input, Arc::clone(&ctx.input)),
            "l" | "bqlpf" => push_args!(id, BiQuad, sample_rate, make_lpf_coefficients),
            "linexp" => push_args!(id, Fn5, pure::linexp),
            "linlin" | "project" => push_args!(id, Fn5, pure::linlin),
            "lpf" => push_args!(id, LPF, sample_rate),
            "m" | "metro" => push_args!(id, Metro, sample_rate),
            "m2f" | "midi2freq" | "#" => push_args!(id, Fn1, pure::midi2freq),
            "max" => push_args!(id, Fn2, pure::max),
            "mh" | "metro_hold" => push_args!(id, MetroHold, sample_rate),
            "min" => push_args!(id, Fn2, pure::min),
            "n" | "noise" | "whiteNoise" => push!(id, WhiteNoise),
            "notch" | "bqnotch" => push_args!(id, BiQuad, sample_rate, make_notch_coefficients),
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
            "uniexp" => push_args!(id, Fn3, pure::uniexp),
            "unit" => push_args!(id, Fn1, pure::unit),
            "w" => push_args!(id, Phasor, sample_rate),
            "wah" => push_args!(id, WahPedal, sample_rate),
            "wrap" => push_args!(id, Fn1, pure::wrap),
            _ => match op.parse::<Sample>() {
                Ok(x) => push_args!(id, Constant, x),
                Err(_) => {
                    let tokens = op.split(':').collect::<Vec<_>>();
                    match tokens[0] {
                        "" if tokens.len() > 1 => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(n) => push_args!(id, Dig, n),
                                Err(_) => {
                                    log::warn!("Can't parse {} as depth", x);
                                }
                            },
                            None => unreachable!(),
                        },
                        "dig" => match tokens.get(1) {
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
                        "fbsat" => {
                            let max_delay = tokens
                                .get(1)
                                .and_then(|x| x.parse::<f64>().ok())
                                .unwrap_or(60.0);
                            program.push(Statement {
                                id,
                                op: Box::new(Feedback::with_shaper(
                                    sample_rate,
                                    max_delay,
                                    pure::tanh,
                                )) as Box<dyn Op>,
                            });
                        }
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
                                    && let Some(table) = load_table(path)
                                {
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
                                    let table_name = String::from(tokens[1]);
                                    // Reuse the existing Arc when the table name and size match,
                                    // so a live-recorded buffer survives program reload.
                                    let table = match ctx.tables.get(&table_name) {
                                        Some(existing) if existing.len() == len => {
                                            Arc::clone(existing)
                                        }
                                        _ => {
                                            let mut t = Vec::with_capacity(len);
                                            for _ in 0..len {
                                                t.push(Default::default());
                                            }
                                            Arc::new(t)
                                        }
                                    };
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
                        "limit" => match tokens.get(1) {
                            Some(x) => {
                                push_args!(id, Limit, sample_rate, x.parse::<f64>().unwrap_or(0.1))
                            }
                            None => push_args!(id, Limit, sample_rate, 0.1),
                        },
                        "norm" => match tokens.get(1) {
                            Some(x) => push_args!(id, Normalise, x.parse::<usize>().unwrap_or(256)),
                            None => push_args!(id, Normalise, 256),
                        },
                        "pat" => match tokens.get(1) {
                            Some(pattern) => push_args!(id, PatternValue, pattern),
                            None => push_args!(id, PatternValue, ""),
                        },
                        "gate" => match tokens.get(1) {
                            Some(pattern) => push_args!(id, PatternGate, pattern),
                            None => push_args!(id, PatternGate, ""),
                        },
                        "trig" => match tokens.get(1) {
                            Some(pattern) => push_args!(id, PatternTrigger, pattern),
                            None => push_args!(id, PatternTrigger, ""),
                        },
                        "cpat" => match tokens.get(1) {
                            Some(pattern) => {
                                push_args!(id, ClockedPatternValue, sample_rate, pattern)
                            }
                            None => push_args!(id, ClockedPatternValue, sample_rate, ""),
                        },
                        "cgate" => match tokens.get(1) {
                            Some(pattern) => {
                                push_args!(id, ClockedPatternGate, sample_rate, pattern)
                            }
                            None => push_args!(id, ClockedPatternGate, sample_rate, ""),
                        },
                        "ctrig" => match tokens.get(1) {
                            Some(pattern) => {
                                push_args!(id, ClockedPatternTrigger, sample_rate, pattern)
                            }
                            None => push_args!(id, ClockedPatternTrigger, sample_rate, ""),
                        },
                        "poly" => {
                            log::warn!(
                                "poly without a preceding quotation; compiling to a zero-voice poly."
                            );
                            push_args!(id, Poly, Vec::new());
                        }
                        "" => {
                            // Empty op (blank node) — silently skip.
                        }
                        _ => {
                            log::warn!("Unknown token: {}", op);
                        }
                    }
                }
            },
        }
    }
    false
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
            && let Some(group) = &mut current_group
        {
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
    let stmts = stmts
        .iter()
        .filter(|stmt| !stmt.op.starts_with(';'))
        .cloned()
        .collect::<Vec<_>>();
    let mut result: Vec<TextOp> = Vec::new();
    let mut new_term: Option<Term> = None;
    // Depth of nested bracket groups inside the group being collected.
    let mut bracket_depth = 0usize;
    let mut terms: HashMap<String, Term> = Default::default();
    let mut stack: Vec<TextOp> = stmts;
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
                // A term used directly before a quotation consumer acts as
                // a quotation: wrap its expansion in quote markers.
                let quoted = stack
                    .last()
                    .is_some_and(|next| is_quotation_consumer(&next.op));
                if quoted {
                    stack.push(TextOp {
                        id: 0,
                        op: QUOTE_CLOSE.to_string(),
                    });
                }
                // Push rewrites onto the stack, not result,
                // to have them processed (as may contain further terms).
                for op in rewrite.drain(..).rev() {
                    stack.push(op);
                }
                if quoted {
                    stack.push(TextOp {
                        id: 0,
                        op: QUOTE_OPEN.to_string(),
                    });
                }
            }
        } else if stmt.op.starts_with("[") {
            if let Some(term) = new_term.as_mut() {
                // Nested bracket group: keep it literal, it is reprocessed
                // when the enclosing group is replayed as a quotation.
                bracket_depth += 1;
                term.ops.push(stmt);
            } else {
                new_term = Some(Term {
                    holes: 0,
                    ops: Vec::new(),
                });
                bracket_depth = 0;
                let token: String = stmt.op.chars().skip(1).collect();
                if !token.is_empty() {
                    stack.push(TextOp {
                        id: stmt.id,
                        op: token,
                    });
                }
            }
        } else if stmt.op == "]" {
            if bracket_depth > 0 {
                bracket_depth -= 1;
                if let Some(term) = new_term.as_mut() {
                    term.ops.push(stmt);
                }
            } else if let Some(term) = new_term.take()
                && let Some(op) = stack.pop()
            {
                if is_quotation_consumer(&op.op) {
                    // A bracket group before a quotation consumer is a
                    // quotation, not a template definition: replay its body
                    // wrapped in quote markers so the compiler can extract
                    // it (and expand templates inside it on the way).
                    stack.push(op);
                    stack.push(TextOp {
                        id: 0,
                        op: QUOTE_CLOSE.to_string(),
                    });
                    for op in term.ops.into_iter().rev() {
                        stack.push(op);
                    }
                    stack.push(TextOp {
                        id: 0,
                        op: QUOTE_OPEN.to_string(),
                    });
                } else {
                    terms.insert(op.op, term);
                }
            }
        } else if stmt.op.ends_with("]") && !stmt.op.contains(':') {
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

fn note_name_semitone(note: char) -> Option<i32> {
    match note.to_ascii_lowercase() {
        'c' => Some(0),
        'd' => Some(2),
        'e' => Some(4),
        'f' => Some(5),
        'g' => Some(7),
        'a' => Some(9),
        'b' => Some(11),
        _ => None,
    }
}

fn parse_ratio_constant(token: &str) -> Option<Sample> {
    let (numerator, denominator) = token.split_once('/')?;
    if denominator.contains('/') || numerator.is_empty() || denominator.is_empty() {
        return None;
    }

    let numerator = numerator.parse::<Sample>().ok()?;
    let denominator = denominator.parse::<Sample>().ok()?;
    Some(pure::safe_div(numerator, denominator))
}

fn parse_note_constant(token: &str) -> Option<Sample> {
    let mut chars = token.chars();
    let note = chars.next()?;
    if !note.is_ascii_alphabetic() || note.eq_ignore_ascii_case(&'s') {
        return None;
    }

    let frequency = note.is_ascii_lowercase();
    let mut semitone = note_name_semitone(note)?;
    let mut rest = chars.as_str();

    if let Some(accidental) = rest.chars().next() {
        match accidental {
            '#' => {
                semitone += 1;
                rest = &rest[accidental.len_utf8()..];
            }
            'b' => {
                semitone -= 1;
                rest = &rest[accidental.len_utf8()..];
            }
            _ => {}
        }
    }

    let octave = rest.parse::<i32>().ok()?;
    let midi = ((octave + 1) * 12 + semitone) as Sample;
    Some(if frequency {
        pure::midi2freq(midi)
    } else {
        midi
    })
}

fn optimized_op(stmt: &TextOp) -> OptimizedOp {
    match stmt.op.parse::<Sample>() {
        Ok(value) => OptimizedOp::Constant { id: stmt.id, value },
        Err(_) => match parse_ratio_constant(&stmt.op).or_else(|| parse_note_constant(&stmt.op)) {
            Some(value) => OptimizedOp::Constant { id: stmt.id, value },
            None => OptimizedOp::Text(stmt.clone()),
        },
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
        ("+" | "add", _, Some(value)) => {
            Some([a.clone(), OptimizedOp::AddConst { id: op.id, value }])
        }
        ("*" | "mul", _, Some(value)) => {
            Some([a.clone(), OptimizedOp::MulConst { id: op.id, value }])
        }
        ("-" | "sub", _, Some(value)) => {
            Some([a.clone(), OptimizedOp::SubConst { id: op.id, value }])
        }
        ("/" | "div", _, Some(value)) => {
            Some([a.clone(), OptimizedOp::DivConst { id: op.id, value }])
        }
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
            rewrite_terms(&[
                TextOp {
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
                }
            ]),
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
            optimize_terms(&[op(1, "2"), op(2, "3"), op(3, "+"), op(4, "4"), op(5, "*"),]),
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
    fn optimize_terms_parses_note_constants() {
        assert_eq!(optimize_terms(&[op(1, "C4")]), vec![constant(1, 60.0)]);
        assert_eq!(optimize_terms(&[op(1, "A4")]), vec![constant(1, 69.0)]);
        assert_eq!(optimize_terms(&[op(1, "a4")]), vec![constant(1, 440.0)]);
        assert_eq!(optimize_terms(&[op(1, "C#4")]), vec![constant(1, 61.0)]);
        assert_eq!(
            optimize_terms(&[op(1, "db4")]),
            vec![constant(1, pure::midi2freq(61.0))]
        );
    }

    #[test]
    fn optimize_terms_parses_ratio_literals() {
        assert_eq!(optimize_terms(&[op(1, "5/4")]), vec![constant(1, 1.25)]);
        assert_eq!(optimize_terms(&[op(1, "1.5/4")]), vec![constant(1, 0.375)]);
        assert_eq!(optimize_terms(&[op(1, "1/0")]), vec![constant(1, 0.0)]);
        assert_eq!(optimize_terms(&[op(1, "1/2/3")]), vec![text(1, "1/2/3")]);
    }

    #[test]
    fn optimize_terms_specializes_binary_ops_with_constants() {
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "0.5"), op(3, "*")]),
            vec![
                text(1, "input"),
                OptimizedOp::MulConst { id: 3, value: 0.5 }
            ]
        );
        assert_eq!(
            optimize_terms(&[op(1, "0.5"), op(2, "input"), op(3, "*")]),
            vec![constant(1, 0.5), text(2, "input"), text(3, "*")]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "+")]),
            vec![
                text(1, "input"),
                OptimizedOp::AddConst { id: 3, value: 2.0 }
            ]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "-")]),
            vec![
                text(1, "input"),
                OptimizedOp::SubConst { id: 3, value: 2.0 }
            ]
        );
        assert_eq!(
            optimize_terms(&[op(1, "2"), op(2, "input"), op(3, "-")]),
            vec![constant(1, 2.0), text(2, "input"), text(3, "-")]
        );
        assert_eq!(
            optimize_terms(&[op(1, "input"), op(2, "2"), op(3, "/")]),
            vec![
                text(1, "input"),
                OptimizedOp::DivConst { id: 3, value: 2.0 }
            ]
        );
        assert_eq!(
            optimize_terms(&[op(1, "2"), op(2, "input"), op(3, "/")]),
            vec![constant(1, 2.0), text(2, "input"), text(3, "/")]
        );
    }

    #[test]
    fn optimize_terms_does_not_steal_constants_used_by_previous_op() {
        assert_eq!(
            optimize_terms(&[
                op(1, "input"),
                op(2, "0.0625"),
                op(3, "5"),
                op(4, "range"),
                op(5, "0.5"),
                op(6, "fb"),
                op(7, "+"),
                op(8, "0.1"),
                op(9, "*"),
            ]),
            vec![
                text(1, "input"),
                constant(2, 0.0625),
                constant(3, 5.0),
                text(4, "range"),
                constant(5, 0.5),
                text(6, "fb"),
                text(7, "+"),
                OptimizedOp::MulConst { id: 9, value: 0.1 },
            ]
        );
    }

    fn run_once(ops: &[TextOp], context: &mut Context) -> Frame {
        let mut vm = audio_vm::VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(compile_program(ops, 100, context));
        vm.play();
        vm.next_frame()
    }

    fn run_frames(ops: &[TextOp], sample_rate: u32, frames: usize) -> Vec<Frame> {
        let mut context = Context::new();
        let mut vm = audio_vm::VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(compile_program(ops, sample_rate, &mut context));
        vm.play();
        (0..frames).map(|_| vm.next_frame()).collect()
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
    fn compile_program_ignores_blank_ops_and_supports_dig_shorthand() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[op(1, "1"), op(2, ""), op(3, "2"), op(4, "+")],
                &mut context
            ),
            [3.0, 3.0]
        );
        assert_eq!(
            run_once(&[op(1, "1"), op(2, "2"), op(3, ":2")], &mut context),
            [1.0, 1.0]
        );
    }

    #[test]
    fn compile_program_stops_at_return_op() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[op(1, "1"), op(2, "return"), op(3, "2"), op(4, "+")],
                &mut context
            ),
            [1.0, 1.0]
        );
        assert_eq!(
            run_once(
                &[op(1, "3"), op(2, "ret"), op(3, "4"), op(4, "*")],
                &mut context
            ),
            [3.0, 3.0]
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
    fn compile_program_drops_comment_tokens() {
        let mut context = Context::new();
        assert_eq!(
            run_once(
                &[
                    op(1, "440"),
                    op(2, ";hello"),
                    op(3, "s"),
                    op(4, ";world-this-is-a-comment"),
                ],
                &mut context,
            ),
            run_once(&[op(1, "440"), op(3, "s")], &mut Context::new())
        );
        assert_eq!(
            run_once(
                &[op(1, "1"), op(2, ";"), op(3, "2"), op(4, "+")],
                &mut context
            ),
            [3.0, 3.0]
        );
    }

    #[test]
    fn comments_inside_templates_and_quotations_are_ignored() {
        let mut context = Context::new();
        assert_eq!(
            run_once(
                &[
                    op(1, "["),
                    op(2, ";ignored"),
                    op(3, "1"),
                    op(4, "]"),
                    op(5, "one"),
                    op(6, "one"),
                ],
                &mut context,
            ),
            [1.0, 1.0]
        );
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, ";ignored"),
                    op(5, "+"),
                    op(6, "]"),
                    op(7, "poly:1"),
                ],
                &mut Context::new(),
            ),
            [6.0, 6.0]
        );
    }

    #[test]
    fn compile_program_runs_saturating_feedback() {
        let saturated = run_frames(
            &[op(1, "1"), op(2, "0.01"), op(3, "1.5"), op(4, "fbsat:1")],
            100,
            64,
        );
        assert!(
            saturated
                .iter()
                .flatten()
                .all(|sample| (-1.0..=1.0).contains(sample))
        );

        let plain = run_frames(
            &[op(1, "1"), op(2, "0.01"), op(3, "1.5"), op(4, "fb:1")],
            100,
            8,
        );
        assert!(plain.iter().flatten().any(|sample| *sample > 1.0));

        let low_saturated = run_frames(
            &[
                op(1, "0.000001"),
                op(2, "0.01"),
                op(3, "0.5"),
                op(4, "fbsat:1"),
            ],
            100,
            16,
        );
        let low_plain = run_frames(
            &[
                op(1, "0.000001"),
                op(2, "0.01"),
                op(3, "0.5"),
                op(4, "fb:1"),
            ],
            100,
            16,
        );
        for (saturated, plain) in low_saturated.iter().zip(low_plain) {
            for (saturated, plain) in saturated.iter().zip(plain) {
                assert!((saturated - plain).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn compile_program_runs_phase_pattern_ops() {
        assert_eq!(
            run_frames(
                &[op(1, "1"), op(2, "cycle"), op(3, "pat:10,20,30,40")],
                4,
                5
            ),
            vec![
                [10.0, 10.0],
                [20.0, 20.0],
                [30.0, 30.0],
                [40.0, 40.0],
                [10.0, 10.0],
            ]
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cy"), op(3, "gate:x.")], 4, 3),
            vec![[1.0, 1.0], [1.0, 1.0], [0.0, 0.0]]
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cycle"), op(3, "trig:x...")], 4, 5),
            vec![[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [1.0, 1.0],]
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cy"), op(3, "gate:[x.]*2")], 4, 4),
            vec![[1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cy"), op(3, "gate:x(1,2)")], 4, 4),
            vec![[1.0, 1.0], [1.0, 1.0], [0.0, 0.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn compile_program_runs_clocked_pattern_convenience_ops() {
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cpat:10,20,30,40")], 4, 5),
            run_frames(
                &[op(1, "1"), op(2, "cycle"), op(3, "pat:10,20,30,40")],
                4,
                5
            )
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "cgate:x.")], 4, 3),
            run_frames(&[op(1, "1"), op(2, "cycle"), op(3, "gate:x.")], 4, 3)
        );
        assert_eq!(
            run_frames(&[op(1, "1"), op(2, "ctrig:x...")], 4, 5),
            run_frames(&[op(1, "1"), op(2, "cycle"), op(3, "trig:x...")], 4, 5)
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
    fn compile_program_supports_variable_get_set_sugar() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[op(1, "7"), op(2, "=answer"), op(3, "<answer"), op(4, "+")],
                &mut context
            ),
            [14.0, 14.0]
        );
        assert!(context.variables.contains_key("answer"));
    }

    #[test]
    fn compile_program_supports_variable_move_sugar() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[op(1, "7"), op(2, ">answer"), op(3, "<answer")],
                &mut context
            ),
            [7.0, 7.0]
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
    fn compile_program_reuses_same_sized_named_tables() {
        let mut context = Context::new();
        let ops = [op(1, "0.75"), op(2, "1"), op(3, "wt:loop:0.01")];

        let _ = compile_program(&ops, 100, &mut context);
        let first = Arc::clone(context.tables.get("loop").expect("created table"));
        first[0][0].store(0.5f64.to_bits(), Ordering::Relaxed);
        first[0][1].store((-0.5f64).to_bits(), Ordering::Relaxed);

        let _ = compile_program(&ops, 100, &mut context);
        let second = context.tables.get("loop").expect("reused table");

        assert!(Arc::ptr_eq(&first, second));
        assert_eq!(
            [
                f64::from_bits(second[0][0].load(Ordering::Relaxed)),
                f64::from_bits(second[0][1].load(Ordering::Relaxed)),
            ],
            [0.5, -0.5]
        );
    }

    #[test]
    fn compile_program_runs_poly_quotation_with_latch_and_routing() {
        let mut context = Context::new();

        // Body `+` adds latched value and routed ctl. Constant ctl 1 rises on
        // the first frame: voice 0 latches 5 and receives the held ctl,
        // voice 1 stays silent: (5 + 1) + 0.
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, "+"),
                    op(5, "]"),
                    op(6, "poly:2"),
                ],
                &mut context
            ),
            [6.0, 6.0]
        );
    }

    #[test]
    fn compile_program_accepts_named_template_as_poly_body() {
        let mut context = Context::new();

        assert_eq!(
            run_once(
                &[
                    op(1, "["),
                    op(2, "+"),
                    op(3, "]"),
                    op(4, "lead"),
                    op(5, "5"),
                    op(6, "1"),
                    op(7, "lead"),
                    op(8, "poly:2"),
                ],
                &mut context
            ),
            [6.0, 6.0]
        );
    }

    #[test]
    fn compile_program_supports_nested_poly_quotations() {
        let mut context = Context::new();

        // Outer body: push 0 and 1, run inner poly (latches 0, ctl 1 -> 1),
        // then add it to the outer routed ctl: outer stack [5, 1, 1] -> [5, 2].
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, "0"),
                    op(5, "1"),
                    op(6, "["),
                    op(7, "+"),
                    op(8, "]"),
                    op(9, "poly:1"),
                    op(10, "+"),
                    op(11, "]"),
                    op(12, "poly:1"),
                ],
                &mut context
            ),
            [2.0, 2.0]
        );
    }

    #[test]
    fn compile_program_forgives_invalid_poly_forms() {
        let mut context = Context::new();

        // poly without a quotation: consumes value and ctl, pushes silence.
        assert_eq!(
            run_once(&[op(1, "5"), op(2, "1"), op(3, "poly:4")], &mut context),
            [0.0, 0.0]
        );
        // Empty body.
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, "]"),
                    op(5, "poly:2"),
                ],
                &mut context
            ),
            [0.0, 0.0]
        );
        // Zero voices.
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, "+"),
                    op(5, "]"),
                    op(6, "poly:0"),
                ],
                &mut context
            ),
            [0.0, 0.0]
        );
    }

    #[test]
    fn return_inside_poly_body_stops_body_compilation_only() {
        let mut context = Context::new();

        // Body compiles to just `+` (ret drops the 9); the outer program
        // continues after poly: (5 + 1) + 2.
        assert_eq!(
            run_once(
                &[
                    op(1, "5"),
                    op(2, "1"),
                    op(3, "["),
                    op(4, "+"),
                    op(5, "ret"),
                    op(6, "9"),
                    op(7, "]"),
                    op(8, "poly:1"),
                    op(9, "2"),
                    op(10, "+"),
                ],
                &mut context
            ),
            [8.0, 8.0]
        );
    }

    #[test]
    fn poly_voice_state_survives_program_reload() {
        let ops = [
            op(1, "0"),
            op(2, "1"),
            op(3, "["),
            op(4, "1"),
            op(5, "w"),
            op(6, "]"),
            op(7, "poly:1"),
        ];
        let sample_rate = 100;

        let mut context = Context::new();
        let mut vm = audio_vm::VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(0.0);
        vm.load_program(compile_program(&ops, sample_rate, &mut context));
        vm.play();
        let mut reloaded = vec![vm.next_frame(), vm.next_frame()];
        vm.load_program(compile_program(&ops, sample_rate, &mut context));
        reloaded.push(vm.next_frame());
        reloaded.push(vm.next_frame());

        // The phasor inside the voice body keeps its phase across the
        // reload: the sequence matches an uninterrupted run.
        assert_eq!(run_frames(&ops, sample_rate, 4), reloaded);
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
