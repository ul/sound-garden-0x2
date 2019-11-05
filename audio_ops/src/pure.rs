//! # Numeric functions
//!
//! These functions could be passed to Fn1::new, Fn2::new and so on (depending on arity)
//! to create Ops which just pass their sources through pure transformation.
use audio_vm::Sample;
use std::f64::consts::PI;

// Arithmetics

pub fn add(x: Sample, y: Sample) -> Sample {
    x + y
}

pub fn mul(x: Sample, y: Sample) -> Sample {
    x * y
}

pub fn sub(x: Sample, y: Sample) -> Sample {
    x - y
}

pub fn div(x: Sample, y: Sample) -> Sample {
    x / y
}

pub fn pow(x: Sample, y: Sample) -> Sample {
    x.powf(y)
}

pub fn recip(x: Sample) -> Sample {
    x.recip()
}

/// Round `x` to the nearest `step` multiplicative.
pub fn quantize(x: Sample, step: Sample) -> Sample {
    (x / step).round() * step
}

// Trigonometry

pub fn sin(x: Sample) -> Sample {
    x.sin()
}

pub fn cos(x: Sample) -> Sample {
    x.cos()
}

// Projections

/// Assuming that x varies in the range a..b linearly project it into the range c..d
pub fn linlin(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    (d - c) * (x - a) / (b - a) + c
}

/// Assuming that x varies in the range -1..1 linearly project it into the range a..b
pub fn range(x: Sample, a: Sample, b: Sample) -> Sample {
    linlin(x, -1.0, 1.0, a, b)
}

/// Assuming that x varies in the range -1..1 linearly project it into the range 0..1
pub fn unit(x: Sample) -> Sample {
    range(x, 0.0, 1.0)
}

/// Assuming that x varies in the range -1..1 linearly project it into the range -PI..PI
pub fn circle(x: Sample) -> Sample {
    range(x, -PI, PI)
}

// Oscillators-ready

/// Connect Phasor to Fn1(sine) to generate sine wave
pub fn sine(phase: Sample) -> Sample {
    sin(2.0 * PI * phase)
}

/// Connect Phasor to Fn1(cosine) to generate cosine wave
pub fn cosine(phase: Sample) -> Sample {
    cos(2.0 * std::f64::consts::PI * phase)
}

/// Connect Phasor to Fn1(triangle) to generate symmetric triangle wave
pub fn triangle(phase: Sample) -> Sample {
    let x = 2.0 * phase;
    if x > 0.0 {
        1.0 - x
    } else {
        1.0 + x
    }
}

/// Connect Phasor and module which outputs pulse width (e.g. Constant(0.5))
/// to Fn2(rectangle) to generate rectangle wave
pub fn rectangle(phase: Sample, width: Sample) -> Sample {
    if unit(phase) <= width {
        1.0
    } else {
        -1.0
    }
}

/// Round to the nearest integer value
pub fn round(x: Sample) -> Sample {
    x.round()
}

/// Convert MIDI pitch to frequency in Hz
pub fn midi2freq(x: Sample) -> Sample {
    440.0 * 2.0f64.powf((x - 69.0) / 12.0)
}

/// Convert frequency in Hz to MIDI pitch
pub fn freq2midi(x: Sample) -> Sample {
    69.0 + 12.0 * (x / 440.0).log2()
}

/// Stereo intensity-preserving panner
pub fn pan(l: Sample, r: Sample, c: Sample) -> (Sample, Sample) {
    (
        1.0_f64.min(1.0 - c).sqrt() * l + 0.0_f64.max(-c).sqrt() * r,
        0.0_f64.max(c).sqrt() * l + 1.0_f64.min(1.0 + c).sqrt() * r,
    )
}

/// Chebyshev polynomial of degree 2
/// T_2(x) = 2x^2 - 1
pub fn cheb2(x: Sample) -> Sample {
    2.0 * x.powi(2) - 1.0
}

/// Chebyshev polynomial of degree 3
/// T_3(x) = 4x^3 - 3x
pub fn cheb3(x: Sample) -> Sample {
    4.0 * x.powi(3) - 3.0 * x
}

/// Chebyshev polynomial of degree 4
/// T_4(x) = 8x^4 - 8x^2 + 1
pub fn cheb4(x: Sample) -> Sample {
    let x2 = x * x;
    8.0 * x2 * (x2 - 1.0) + 1.0
}

/// Chebyshev polynomial of degree 5
/// T_5(x) = 16x^5 - 20x^3 +5x
pub fn cheb5(x: Sample) -> Sample {
    let x2 = x * x;
    let x3 = x2 * x;
    16.0 * x2 * x3 - 20.0 * x3 + 5.0 * x
}

/// Chebyshev polynomial of degree 6
/// T_6(x) = 32x^6 - 48x^4 + 18x^2 - 1
pub fn cheb6(x: Sample) -> Sample {
    let x2 = x * x;
    let x4 = x2 * x2;
    32.0 * x2 * x4 - 48.0 * x4 + 18.0 * x2 - 1.0
}
