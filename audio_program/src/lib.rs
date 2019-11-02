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
            "cos" => push_args!(Fn1, pure::cos),
            "m2f" | "midi2freq" => push_args!(Fn1, pure::midi2freq),
            "p" | "pulse" => push_args!(Pulse, sample_rate),
            "s" => push_args!(Osc, sample_rate, pure::sine),
            "saw" => push_args!(Phasor0, sample_rate),
            "sine" => push_args!(OscPhase, sample_rate, pure::sine),
            "t" => push_args!(Osc, sample_rate, pure::triangle),
            "tri" => push_args!(OscPhase, sample_rate, pure::triangle),
            "w" => push_args!(Phasor, sample_rate),
            "dup" => push!(Dup),
            "swap" => push!(Swap),
            "rot" => push!(Rot),
            _ => match token.parse::<Sample>() {
                Ok(x) => push_args!(Constant, x),
                Err(_) => {}
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
