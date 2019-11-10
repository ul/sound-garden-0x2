use audio_ops::*;
use audio_vm::{Op, Program, Sample, CHANNELS};
use fasthash::sea::Hash64;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub fn parse_tokens(tokens: &[String], sample_rate: u32) -> Program {
    let mut ops = SmallVec::new();
    macro_rules! push {
        ( $class:ident ) => {
            ops.push(Box::new($class::new()) as Box<dyn Op + Send>)
        };
    }
    macro_rules! push_args {
        ( $class:ident, $($rest:tt)* ) => {
            ops.push(Box::new($class::new($($rest)*)) as Box<dyn Op+Send>)
        };
    }
    let mut tables = HashMap::with_hasher(Hash64);
    for token in tokens {
        match token.as_str() {
            "*" => push_args!(Fn2, pure::mul),
            "+" => push_args!(Fn2, pure::add),
            "-" => push_args!(Fn2, pure::sub),
            "/" => push_args!(Fn2, pure::div),
            "\\" => push_args!(Fn1, pure::recip),
            "^" | "pow" => push_args!(Fn2, pure::pow),
            "cheb2" => push_args!(Fn1, pure::cheb2),
            "cheb3" => push_args!(Fn1, pure::cheb3),
            "cheb4" => push_args!(Fn1, pure::cheb4),
            "cheb5" => push_args!(Fn1, pure::cheb5),
            "cheb6" => push_args!(Fn1, pure::cheb6),
            "circle" => push_args!(Fn1, pure::circle),
            "cos" => push_args!(Fn1, pure::cos),
            "dm" | "dmetro" => push_args!(DMetro, sample_rate),
            "dmh" | "dmetro_hold" => push_args!(DMetroHold, sample_rate),
            "dup" => push!(Dup),
            "h" | "bqhpf" => push_args!(BiQuad, sample_rate, make_hpf_coefficients),
            "hpf" => push_args!(HPF, sample_rate),
            "f2m" | "freq2midi" => push_args!(Fn1, pure::freq2midi),
            "impulse" => push_args!(Impulse, sample_rate),
            "l" | "bqlpf" => push_args!(BiQuad, sample_rate, make_lpf_coefficients),
            "lpf" => push_args!(LPF, sample_rate),
            "m2f" | "midi2freq" => push_args!(Fn1, pure::midi2freq),
            "m" | "metro" => push_args!(Metro, sample_rate),
            "mh" | "metro_hold" => push_args!(MetroHold, sample_rate),
            "n" | "noise" => push!(WhiteNoise),
            "p" | "pulse" => push_args!(Pulse, sample_rate),
            "pan1" => push!(Pan1),
            "pan2" => push!(Pan2),
            "panx" => push!(Pan3),
            "pop" => push!(Pop),
            "q" | "quantize" => push_args!(Fn2, pure::quantize),
            "r" | "range" => push_args!(Fn3, pure::range),
            "round" => push_args!(Fn1, pure::round),
            "rot" => push!(Rot),
            "s" => push_args!(Osc, sample_rate, pure::sine),
            "sh" | "sample&hold" => push!(SampleAndHold),
            "saw" => push_args!(Phasor0, sample_rate),
            "sin" => push_args!(Fn1, pure::sin),
            "sine" => push_args!(OscPhase, sample_rate, pure::sine),
            "swap" => push!(Swap),
            "t" => push_args!(Osc, sample_rate, pure::triangle),
            "tri" => push_args!(OscPhase, sample_rate, pure::triangle),
            "unit" => push_args!(Fn1, pure::unit),
            "w" => push_args!(Phasor, sample_rate),
            _ => match token.parse::<Sample>() {
                Ok(x) => push_args!(Constant, x),
                Err(_) => {
                    let tokens = token.split(':').collect::<Vec<_>>();
                    match tokens[0] {
                        "dl" | "delay" => match tokens.get(1) {
                            Some(x) => match x.parse::<f64>() {
                                Ok(max_delay) => push_args!(Delay, sample_rate, max_delay),
                                Err(_) => {}
                            },
                            None => {}
                        },
                        "fb" | "feedback" => match tokens.get(1) {
                            Some(x) => match x.parse::<f64>() {
                                Ok(max_delay) => push_args!(Feedback, sample_rate, max_delay),
                                Err(_) => {}
                            },
                            None => {}
                        },
                        "rt" | "rtab" | "readtable" => {
                            match tokens.get(1).and_then(|x| tables.get(*x)) {
                                Some(table) => {
                                    push_args!(TableReader, sample_rate, Arc::clone(table));
                                }
                                None => {}
                            }
                        }
                        "wt" | "wtab" | "writetable" => match tokens.get(2) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(size) => {
                                    let table_name = String::from(tokens[1]);
                                    let table = Arc::new(Mutex::new(vec![
                                        [0.0; CHANNELS];
                                        size * (sample_rate
                                            as usize)
                                    ]));
                                    tables.insert(table_name, Arc::clone(&table));
                                    push_args!(TableWriter, table);
                                }
                                Err(_) => {}
                            },
                            None => {}
                        },
                        "conv" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(window_size) => push_args!(Convolution, window_size),
                                Err(_) => {}
                            },
                            None => {}
                        },
                        "convm" => match tokens.get(1) {
                            Some(x) => match x.parse::<usize>() {
                                Ok(window_size) => push_args!(ConvolutionM, window_size),
                                Err(_) => {}
                            },
                            None => {}
                        },
                        _ => {}
                    }
                }
            },
        }
    }
    ops
}

pub fn parse_program(s: &str, sample_rate: u32) -> Program {
    let s = s.replace(|c| c == '[' || c == ']' || c == ',', " ");
    let tokens = s
        .split_terminator('\n')
        .flat_map(|s| s.splitn(2, "//").take(1).flat_map(|s| s.split_whitespace()))
        .map(|x| String::from(x))
        .collect::<Vec<_>>();
    parse_tokens(&tokens, sample_rate)
}
