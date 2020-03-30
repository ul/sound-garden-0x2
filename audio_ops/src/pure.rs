//! # Numeric functions
//!
//! These functions could be passed to Fn1::new, Fn2::new and so on (depending on arity)
//! to create Ops which just pass their sources through pure transformation.
use audio_vm::Sample;
use std::f64::consts::PI;

// Arithmetics

#[inline]
pub fn add(x: Sample, y: Sample) -> Sample {
    x + y
}

#[inline]
pub fn mul(x: Sample, y: Sample) -> Sample {
    x * y
}

#[inline]
pub fn sub(x: Sample, y: Sample) -> Sample {
    x - y
}

#[inline]
pub fn div(x: Sample, y: Sample) -> Sample {
    x / y
}

#[inline]
pub fn safe_div(x: Sample, y: Sample) -> Sample {
    if y != 0.0 {
        x / y
    } else {
        0.0
    }
}

#[inline]
pub fn modulo(x: Sample, y: Sample) -> Sample {
    x % y
}

#[inline]
pub fn pow(x: Sample, y: Sample) -> Sample {
    x.powf(y)
}

#[inline]
pub fn recip(x: Sample) -> Sample {
    x.recip()
}

#[inline]
pub fn safe_recip(x: Sample) -> Sample {
    if x != 0.0 {
        x.recip()
    } else {
        0.0
    }
}

/// Round `x` to the nearest `step` multiplicative.
#[inline]
pub fn quantize(x: Sample, step: Sample) -> Sample {
    (x / step).round() * step
}

// Trigonometry

#[inline]
pub fn sin(x: Sample) -> Sample {
    micromath::F32Ext::sin(x as f32) as _
}

#[inline]
pub fn cos(x: Sample) -> Sample {
    micromath::F32Ext::cos(x as f32) as _
}

#[inline]
pub fn tan(x: Sample) -> Sample {
    micromath::F32Ext::tan(x as f32) as _
}

#[inline]
pub fn sinh(x: Sample) -> Sample {
    x.sinh()
}

#[inline]
pub fn cosh(x: Sample) -> Sample {
    x.cosh()
}

#[inline]
pub fn tanh(x: Sample) -> Sample {
    x.tanh()
}

// Projections

/// Assuming that x varies in the range a..b linearly project it into the range c..d
#[inline]
pub fn linlin(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    (d - c) * (x - a) / (b - a) + c
}

/// Assuming that x varies in the range -1..1 linearly project it into the range a..b
#[inline]
pub fn range(x: Sample, a: Sample, b: Sample) -> Sample {
    linlin(x, -1.0, 1.0, a, b)
}

/// Assuming that x varies in the range -1..1 linearly project it into the range 0..1
#[inline]
pub fn unit(x: Sample) -> Sample {
    range(x, 0.0, 1.0)
}

/// Assuming that x varies in the range -1..1 linearly project it into the range -PI..PI
#[inline]
pub fn circle(x: Sample) -> Sample {
    range(x, -PI, PI)
}

// Oscillators-ready

/// Connect Phasor to Fn1(sine) to generate sine wave
#[inline]
pub fn sine(phase: Sample) -> Sample {
    sin(2.0 * PI * phase)
}

/// Connect Phasor to Fn1(cosine) to generate cosine wave
#[inline]
pub fn cosine(phase: Sample) -> Sample {
    cos(2.0 * PI * phase)
}

/// Connect Phasor to Fn1(triangle) to generate symmetric triangle wave
#[inline]
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
#[inline]
pub fn rectangle(phase: Sample, width: Sample) -> Sample {
    if unit(phase) <= width {
        1.0
    } else {
        -1.0
    }
}

/// Round to the nearest integer value
#[inline]
pub fn round(x: Sample) -> Sample {
    x.round()
}

/// Convert MIDI pitch to frequency in Hz
#[inline]
pub fn midi2freq(x: Sample) -> Sample {
    440.0 * 2.0f64.powf((x - 69.0) / 12.0)
}

/// Convert frequency in Hz to MIDI pitch
#[inline]
pub fn freq2midi(x: Sample) -> Sample {
    69.0 + 12.0 * (x / 440.0).log2()
}

/// Stereo intensity-preserving panner
#[inline]
pub fn pan(l: Sample, r: Sample, c: Sample) -> (Sample, Sample) {
    (
        1.0_f64.min(1.0 - c).sqrt() * l + 0.0_f64.max(-c).sqrt() * r,
        0.0_f64.max(c).sqrt() * l + 1.0_f64.min(1.0 + c).sqrt() * r,
    )
}

/// Chebyshev polynomial of degree 2
/// T_2(x) = 2x^2 - 1
#[inline]
pub fn cheb2(x: Sample) -> Sample {
    2.0 * x.powi(2) - 1.0
}

/// Chebyshev polynomial of degree 3
/// T_3(x) = 4x^3 - 3x
#[inline]
pub fn cheb3(x: Sample) -> Sample {
    4.0 * x.powi(3) - 3.0 * x
}

/// Chebyshev polynomial of degree 4
/// T_4(x) = 8x^4 - 8x^2 + 1
#[inline]
pub fn cheb4(x: Sample) -> Sample {
    let x2 = x * x;
    8.0 * x2 * (x2 - 1.0) + 1.0
}

/// Chebyshev polynomial of degree 5
/// T_5(x) = 16x^5 - 20x^3 +5x
#[inline]
pub fn cheb5(x: Sample) -> Sample {
    let x2 = x * x;
    let x3 = x2 * x;
    16.0 * x2 * x3 - 20.0 * x3 + 5.0 * x
}

/// Chebyshev polynomial of degree 6
/// T_6(x) = 32x^6 - 48x^4 + 18x^2 - 1
#[inline]
pub fn cheb6(x: Sample) -> Sample {
    let x2 = x * x;
    let x4 = x2 * x2;
    32.0 * x2 * x4 - 48.0 * x4 + 18.0 * x2 - 1.0
}

#[inline]
pub fn clamp(x: Sample, min: Sample, max: Sample) -> Sample {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

// Convert decibels to amplitude.
#[inline]
pub fn db2amp(x: Sample) -> Sample {
    20.0 * x.log10()
}

// Convert amplitude to decibels.
#[inline]
pub fn amp2db(x: Sample) -> Sample {
    10.0f64.powf(x / 20.0)
}

#[inline]
pub fn min(x: Sample, y: Sample) -> Sample {
    x.min(y)
}

#[inline]
pub fn max(x: Sample, y: Sample) -> Sample {
    x.max(y)
}

#[inline]
pub fn clip(x: Sample) -> Sample {
    if x < -1.0 {
        -1.0
    } else if 1.0 < x {
        1.0
    } else {
        x
    }
}

#[inline]
pub fn wrap(x: Sample) -> Sample {
    (x + 1.0) % 2.0 - 1.0
}

#[inline]
pub fn exp(x: Sample) -> Sample {
    x.exp()
}

#[inline]
pub fn linear_curve(a: Sample, dx: Sample) -> Sample {
    a * dx
}

#[inline]
pub fn quadratic_curve(a: Sample, dx: Sample) -> Sample {
    a.powi(4) * dx
}
