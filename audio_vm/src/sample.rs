pub const CHANNELS: usize = 2;

/// The type which Ops talk to each other and to audio driver.
/// Rationale behind choosing f64 over f32 despite the fact that most of audio drivers work with f32
/// is that when signal goes through AudioGraph it experiences a lot of transformations and the less
/// is rounding error accumulation is better. Regarding performance, The Book says: "The default
/// type is f64 because on modern CPUs it’s roughly the same speed as f32 but is capable of more
/// precision."
pub type Sample = f64;

/// Snapshot of multi-channel signal output at specific point of time.
pub type Frame = [Sample; CHANNELS];
