use audio_program::{Context, TextOp, compile_program};
use audio_vm::VM;

const SAMPLE_RATE: u32 = 48_000;

fn render(source: &str, frames: usize) -> Vec<f64> {
    let ops = source
        .split_whitespace()
        .enumerate()
        .map(|(index, op)| TextOp {
            id: (index + 1) as u64,
            op: op.to_owned(),
        })
        .collect::<Vec<_>>();
    let mut vm = VM::new();
    vm.set_xfade_duration(0.0);
    vm.load_program(compile_program(&ops, SAMPLE_RATE, &mut Context::new()));
    vm.play();
    (0..frames).map(|_| vm.next_frame()[0]).collect()
}

/// Rising crossings with hysteresis, so polyBLEP ripple near an edge can't
/// be counted twice.
fn cycles(signal: &[f64]) -> usize {
    let mut armed = false;
    let mut count = 0;
    for &x in signal {
        if x < -0.5 {
            armed = true;
        } else if armed && x > 0.5 {
            armed = false;
            count += 1;
        }
    }
    count
}

#[test]
fn every_oscillator_completes_one_cycle_per_period() {
    // `{f}` is replaced by a constant (folded into FixedOsc where possible) and
    // by a variable read (never folded), so both code paths are covered.
    let templates = [
        "{f} s",
        "{f} s'",
        "{f} c",
        "{f} c'",
        "{f} 0 sine",
        "{f} 0 sine'",
        "{f} 0 cosine",
        "{f} 0 cosine'",
        "{f} w",
        "{f} 0 saw",
        "{f} 0 saw'",
        "{f} t",
        "{f} t'",
        "{f} 0 tri",
        "{f} 0 tri'",
        "{f} 0.5 p",
        "{f} 0.5 p'",
        "{f} 0.5 0 pulse",
        "{f} 0.5 0 pulse'",
    ];
    for template in templates {
        for frequency in ["110", "110 >f <f"] {
            let source = template.replace("{f}", frequency);
            // One second: the count is the frequency, give or take the partial
            // cycle at either end.
            let count = cycles(&render(&source, SAMPLE_RATE as usize));
            assert!(
                (109..=111).contains(&count),
                "{source}: {count} cycles per second, expected 110"
            );
        }
    }
}

#[test]
fn phase_offset_of_two_is_one_full_cycle_for_every_oscillator() {
    for (base, shifted) in [
        ("0 sine", "2 sine"),
        ("0 cosine", "2 cosine"),
        ("0 saw", "2 saw"),
        ("0 tri", "2 tri"),
        ("0.5 0 pulse", "0.5 2 pulse"),
    ] {
        let a = render(&format!("110 {base}"), 4800);
        let b = render(&format!("110 {shifted}"), 4800);
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-9, "{base} vs {shifted}: {x} != {y}");
        }
    }
}
