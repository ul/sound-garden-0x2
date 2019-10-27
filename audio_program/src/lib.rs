use audio_ops::*;
use audio_vm::{Op, Sample, VM};
use smallvec::SmallVec;

macro_rules! connect0 {
    ( $ops:ident, $class:ident ) => {
        $ops.push(Box::new($class::new()) as Box<dyn Op + Send>)
    };
}

macro_rules! connect {
    ( $ops:ident, $class:ident, $($rest:tt)* ) => {
        $ops.push(Box::new($class::new($($rest)*)) as Box<dyn Op+Send>)
    };
}

pub fn parse_program(s: &str, sample_rate: u32) -> VM {
    let mut vm = VM::new();
    let mut ops = SmallVec::new();
    let s = s.replace(|c| c == '[' || c == ']' || c == ',', " ");
    for token in s
        .split_terminator('\n')
        .flat_map(|s| s.splitn(2, "//").take(1).flat_map(|s| s.split_whitespace()))
    {
        match token {
            "*" => connect!(ops, Fn2, pure::mul),
            "+" => connect!(ops, Fn2, pure::add),
            "-" => connect!(ops, Fn2, pure::sub),
            "/" => connect!(ops, Fn2, pure::div),
            "\\" => connect!(ops, Fn1, pure::recip),
            "^" | "pow" => connect!(ops, Fn2, pure::pow),
            "cheb2" => connect!(ops, Fn1, pure::cheb2),
            "cheb3" => connect!(ops, Fn1, pure::cheb3),
            "cheb4" => connect!(ops, Fn1, pure::cheb4),
            "cheb5" => connect!(ops, Fn1, pure::cheb5),
            "cheb6" => connect!(ops, Fn1, pure::cheb6),
            "cos" => connect!(ops, Fn1, pure::cos),
            "m2f" | "midi2freq" => connect!(ops, Fn1, pure::midi2freq),
            "p" | "pulse" => connect!(ops, Pulse, sample_rate),
            "s" => connect!(ops, Osc, sample_rate, pure::sine),
            "saw" => connect!(ops, Phasor0, sample_rate),
            "sine" => connect!(ops, OscPhase, sample_rate, pure::sine),
            "t" => connect!(ops, Osc, sample_rate, pure::triangle),
            "tri" => connect!(ops, OscPhase, sample_rate, pure::triangle),
            "w" => connect!(ops, Phasor, sample_rate),
            "dup" => connect0!(ops, Dup),
            "swap" => connect0!(ops, Swap),
            "rot" => connect0!(ops, Rot),
            _ => match token.parse::<Sample>() {
                Ok(x) => connect!(ops, Constant, x),
                Err(_) => {}
            },
        }
    }
    vm.load_program(ops);
    vm
}
