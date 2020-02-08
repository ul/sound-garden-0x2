use audio_ops::*;
use audio_vm::{Frame, Op, Program, Sample, Statement, CHANNELS};
use fasthash::sea::Hash64;
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use regex::Regex;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const HELP: &str = include_str!("help.adoc");

pub struct Context {
    pub tables: HashMap<String, Arc<Mutex<Vec<Frame>>>, Hash64>,
}

impl Context {
    pub fn new() -> Self {
        Context {
            tables: HashMap::with_hasher(Hash64),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Context::new()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextOp {
    pub id: u64,
    pub op: String,
}

pub fn compile_program(ops: &[TextOp], sample_rate: u32, ctx: &mut Context) -> Program {
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
    for TextOp { id, op } in ops {
        let id = *id;
        match op.as_str() {
            "*" | "mul" => push_args!(id, Fn2, pure::mul),
            "+" | "add" => push_args!(id, Fn2, pure::add),
            "-" | "sub" => push_args!(id, Fn2, pure::sub),
            "/" | "div" => push_args!(id, Fn2, pure::div),
            "\\" => push_args!(id, Fn1, pure::recip),
            "^" | "pow" => push_args!(id, Fn2, pure::pow),
            "adsr" => push_args!(id, ADSR, sample_rate),
            "amp2db" | "a2db" => push_args!(id, Fn1, pure::amp2db),
            "c" => push_args!(id, Osc, sample_rate, pure::cosine),
            "cheb2" => push_args!(id, Fn1, pure::cheb2),
            "cheb3" => push_args!(id, Fn1, pure::cheb3),
            "cheb4" => push_args!(id, Fn1, pure::cheb4),
            "cheb5" => push_args!(id, Fn1, pure::cheb5),
            "cheb6" => push_args!(id, Fn1, pure::cheb6),
            "circle" => push_args!(id, Fn1, pure::circle),
            "clamp" => push_args!(id, Fn3, pure::clamp),
            "clip" => push_args!(id, Fn1, pure::clip),
            "cos" => push_args!(id, Fn1, pure::cos),
            "cosh" => push_args!(id, Fn1, pure::cosh),
            "cosine" => push_args!(id, OscPhase, sample_rate, pure::cosine),
            "db2amp" | "db2a" => push_args!(id, Fn1, pure::db2amp),
            "dm" | "dmetro" => push_args!(id, DMetro, sample_rate),
            "dmh" | "dmetro_hold" => push_args!(id, DMetroHold, sample_rate),
            "dup" => push!(id, Dup),
            "exp" => push_args!(id, Fn1, pure::exp),
            "f2m" | "freq2midi" => push_args!(id, Fn1, pure::freq2midi),
            "h" | "bqhpf" => push_args!(id, BiQuad, sample_rate, make_hpf_coefficients),
            "hpf" => push_args!(id, HPF, sample_rate),
            "impulse" => push_args!(id, Impulse, sample_rate),
            "l" | "bqlpf" => push_args!(id, BiQuad, sample_rate, make_lpf_coefficients),
            "linlin" | "project" => push_args!(id, Fn5, pure::linlin),
            "lpf" => push_args!(id, LPF, sample_rate),
            "m" | "metro" => push_args!(id, Metro, sample_rate),
            "m2f" | "midi2freq" => push_args!(id, Fn1, pure::midi2freq),
            "max" => push_args!(id, Fn2, pure::max),
            "mh" | "metro_hold" => push_args!(id, MetroHold, sample_rate),
            "min" => push_args!(id, Fn2, pure::min),
            "n" | "noise" | "whiteNoise" => push!(id, WhiteNoise),
            "p" => push_args!(id, Pulse, sample_rate),
            "pan1" => push!(id, Pan1),
            "pan2" => push!(id, Pan2),
            "panx" => push!(id, Pan3),
            "pitch" => push_args!(id, Yin, sample_rate, 1024, 64, 0.2),
            "pop" => push!(id, Pop),
            "prime" => push!(id, Prime),
            "pulse" => push_args!(id, PulsePhase, sample_rate),
            "q" | "quantize" => push_args!(id, Fn2, pure::quantize),
            "r" | "range" => push_args!(id, Fn3, pure::range),
            "rot" => push!(id, Rot),
            "round" => push_args!(id, Fn1, pure::round),
            "s" => push_args!(id, Osc, sample_rate, pure::sine),
            "saw" => push_args!(id, Phasor0, sample_rate),
            "sh" | "sample&hold" => push!(id, SampleAndHold),
            "ssh" => push!(id, SmoothSampleAndHold),
            "silence" => push_args!(id, Constant, 0.0),
            "sin" => push_args!(id, Fn1, pure::sin),
            "sine" => push_args!(id, OscPhase, sample_rate, pure::sine),
            "sinh" => push_args!(id, Fn1, pure::sinh),
            "spectral_shuffle" => {
                let mut rng = Box::new(SmallRng::from_entropy());
                push_args!(
                    id,
                    SpectralTransform,
                    2048, // window_size
                    64,   // period
                    Box::new(move |freqs| freqs.shuffle(&mut rng)),
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
            "swap" => push!(id, Swap),
            "t" => push_args!(id, Osc, sample_rate, pure::triangle),
            "tan" => push_args!(id, Fn1, pure::tan),
            "tanh" => push_args!(id, Fn1, pure::tanh),
            "tri" => push_args!(id, OscPhase, sample_rate, pure::triangle),
            "unit" => push_args!(id, Fn1, pure::unit),
            "w" => push_args!(id, Phasor, sample_rate),
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
                                    let table_name = String::from(tokens[1]);
                                    let table = Arc::new(Mutex::new(vec![
                                        [0.0; CHANNELS];
                                        (size * (sample_rate as Sample))
                                            as _
                                    ]));
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

pub fn rewrite_terms(stmts: &[TextOp]) -> Vec<TextOp> {
    let mut result: Vec<TextOp> = Vec::new();
    let mut new_term: Option<Term> = None;
    let mut terms: HashMap<String, Term> = Default::default();
    let mut stack: Vec<TextOp> = Vec::from(stmts.clone());
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
            if let Some(term) = new_term.take() {
                if let Some(op) = stack.pop() {
                    terms.insert(op.op, term);
                }
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

pub fn get_help() -> HashMap<String, String> {
    let mut result = HashMap::new();
    for item in Regex::new(r"(?P<term>(\w+(:<\w+>)?(, )*)+)::(?P<definition>.+)")
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
    let item_re = Regex::new(r"(?P<term>(\w+(:<\w+>)?(, )*)+)::").unwrap();
    let mut current_group = None;
    for line in HELP.split('\n') {
        if let Some(m) = group_re.captures(line) {
            if let Some(group) = current_group {
                result.push(group);
            }
            current_group = Some((m.get(1).unwrap().as_str().to_owned(), Vec::new()));
        } else if let Some(m) = item_re.captures(line) {
            if let Some(group) = &mut current_group {
                group.1.extend(
                    m.name("term")
                        .unwrap()
                        .as_str()
                        .split(", ")
                        .map(|x| x.to_owned()),
                );
            }
        }
    }
    result
}

struct Term {
    holes: usize,
    ops: Vec<TextOp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_terms_does_its_thing() {
        assert_eq!(
            rewrite_terms(&vec![
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
                },
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
                }
            ]
        );
    }
}
