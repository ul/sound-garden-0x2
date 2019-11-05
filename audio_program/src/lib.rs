use audio_ops::*;
use audio_vm::{Op, Program, Sample};
use smallvec::SmallVec;

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
            "f2m" | "freq2midi" => push_args!(Fn1, pure::freq2midi),
            "impulse" => push_args!(Impulse, sample_rate),
            "l" | "bqlpf" => push_args!(BiQuad, sample_rate, make_lpf_coefficients),
            "m2f" | "midi2freq" => push_args!(Fn1, pure::midi2freq),
            "m" | "metro" => push_args!(Metro, sample_rate),
            "mh" | "metro_hold" => push_args!(MetroHold, sample_rate),
            "n" | "noise" => push!(WhiteNoise),
            "p" | "pulse" => push_args!(Pulse, sample_rate),
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
