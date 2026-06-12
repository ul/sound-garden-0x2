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
    if y != 0.0 { x / y } else { 0.0 }
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
    if x != 0.0 { x.recip() } else { 0.0 }
}

/// Round `x` to the nearest `step` multiplicative.
#[inline]
pub fn quantize(x: Sample, step: Sample) -> Sample {
    safe_div(x, step).round() * step
}

// Trigonometry

#[inline]
pub fn sin(x: Sample) -> Sample {
    x.sin()
}

#[inline]
pub fn cos(x: Sample) -> Sample {
    x.cos()
}

#[inline]
pub fn tan(x: Sample) -> Sample {
    x.tan()
}

#[inline]
pub fn sin_fast(x: Sample) -> Sample {
    micromath::F32Ext::sin(x as f32) as _
}

#[inline]
pub fn cos_fast(x: Sample) -> Sample {
    micromath::F32Ext::cos(x as f32) as _
}

#[inline]
pub fn tan_fast(x: Sample) -> Sample {
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

#[inline]
pub fn drive(x: Sample, amount: Sample) -> Sample {
    let amount = amount.max(0.001);
    (x * amount).tanh() / amount.tanh()
}

#[inline]
pub fn fold(x: Sample, amount: Sample) -> Sample {
    let folded = x * amount.max(1.0);
    1.0 - ((folded + 1.0).rem_euclid(4.0) - 2.0).abs()
}

// Projections

#[inline]
fn finite_or_zero(x: Sample) -> Sample {
    if x.is_finite() { x } else { 0.0 }
}

/// Assuming that x varies in the range a..b linearly project it into the range c..d
#[inline]
pub fn linlin(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    finite_or_zero(safe_div((d - c) * (x - a), b - a) + c)
}

/// Assuming that x varies in the range 0..1 exponentially project it into the range lo..hi
#[inline]
pub fn uniexp(x: Sample, lo: Sample, hi: Sample) -> Sample {
    finite_or_zero(lo * safe_div(hi, lo).powf(x))
}

/// Assuming that x varies in the range -1..1 exponentially project it into the range lo..hi
#[inline]
pub fn biexp(x: Sample, lo: Sample, hi: Sample) -> Sample {
    uniexp(unit(x), lo, hi)
}

/// Assuming that x varies in the range a..b linearly project it into the exponential range c..d
#[inline]
pub fn linexp(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    uniexp(safe_div(x - a, b - a), c, d)
}

/// Assuming that x varies in the exponential range a..b, project it into the linear range c..d
#[inline]
pub fn explin(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    linlin(safe_div(x, a).log2(), 0.0, safe_div(b, a).log2(), c, d)
}

/// Assuming that x varies in the exponential range a..b, project it into the exponential range c..d
#[inline]
pub fn expexp(x: Sample, a: Sample, b: Sample, c: Sample, d: Sample) -> Sample {
    uniexp(safe_div(safe_div(x, a).log2(), safe_div(b, a).log2()), c, d)
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

/// Connect Phasor to Fn1(sine) to generate sine wave
#[inline]
pub fn sine_fast(phase: Sample) -> Sample {
    sin_fast(2.0 * PI * phase)
}

/// Connect Phasor to Fn1(cosine) to generate cosine wave
#[inline]
pub fn cosine_fast(phase: Sample) -> Sample {
    cos_fast(2.0 * PI * phase)
}

/// Connect Phasor to Fn1(triangle) to generate symmetric triangle wave
#[inline]
pub fn triangle(phase: Sample) -> Sample {
    let x = 2.0 * phase;
    if x > 0.0 { 1.0 - x } else { 1.0 + x }
}

/// Connect Phasor and module which outputs pulse width (e.g. Constant(0.5))
/// to Fn2(rectangle) to generate rectangle wave
#[inline]
pub fn rectangle(phase: Sample, width: Sample) -> Sample {
    if unit(phase) <= width { 1.0 } else { -1.0 }
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

/// Convert decibels to amplitude: 10^(x / 20).
#[inline]
pub fn db2amp(x: Sample) -> Sample {
    10.0f64.powf(x / 20.0)
}

/// Convert amplitude to decibels: 20 * log10(x).
#[inline]
pub fn amp2db(x: Sample) -> Sample {
    20.0 * x.log10()
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
    x.clamp(-1.0, 1.0)
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

#[inline]
pub fn sinc(x: Sample) -> Sample {
    if x != 0.0 { x.sin() / x } else { 1.0 }
}

#[inline]
pub fn sinc_fast(x: Sample) -> Sample {
    if x != 0.0 { sin_fast(x) / x } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: Sample, expected: Sample) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn drive_is_gain_compensated_tanh_saturation() {
        assert_close(drive(1.0, 1_000.0), 1.0);
        assert_close(drive(-1.0, 1_000.0), -1.0);
        let small = 0.01;
        assert!(drive(small, 1.0) >= small);
    }

    #[test]
    fn fold_stays_bounded_and_is_identity_at_unity_for_small_inputs() {
        for x in [-12.3, -3.0, -1.5, -1.0, -0.25, 0.0, 0.5, 1.0, 1.7, 4.2] {
            assert!(
                (-1.0..=1.0).contains(&fold(x, 3.0)),
                "{x} -> {}",
                fold(x, 3.0)
            );
        }
        for x in [-1.0, -0.5, 0.0, 0.25, 1.0] {
            assert_close(fold(x, 1.0), x);
        }
    }

    #[test]
    fn decibel_and_amplitude_conversions_are_named_correctly() {
        assert_close(db2amp(0.0), 1.0);
        assert_close(db2amp(20.0), 10.0);
        assert_close(amp2db(1.0), 0.0);
        assert_close(amp2db(10.0), 20.0);

        for db in [-24.0, -6.0, 0.0, 3.0, 18.0] {
            assert_close(amp2db(db2amp(db)), db);
        }
    }

    #[test]
    fn exponential_projection_ops_map_endpoints_and_midpoints() {
        assert_close(uniexp(0.0, 100.0, 10_000.0), 100.0);
        assert_close(uniexp(0.5, 100.0, 10_000.0), 1_000.0);
        assert_close(uniexp(1.0, 100.0, 10_000.0), 10_000.0);

        assert_close(biexp(-1.0, 100.0, 10_000.0), 100.0);
        assert_close(biexp(0.0, 100.0, 10_000.0), 1_000.0);
        assert_close(biexp(1.0, 100.0, 10_000.0), 10_000.0);

        assert_close(linexp(5.0, 0.0, 10.0, 100.0, 10_000.0), 1_000.0);
        assert_close(explin(1_000.0, 100.0, 10_000.0, 0.0, 10.0), 5.0);
        assert_close(expexp(1_000.0, 100.0, 10_000.0, 1.0, 16.0), 4.0);
    }
}
