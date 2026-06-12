use audio_vm::{CHANNELS, Op, Sample, Stack};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaleQuantizer {
    intervals: Box<[i32]>,
}

impl ScaleQuantizer {
    pub fn new(intervals: &[i32]) -> Option<Self> {
        let mut intervals: Vec<i32> = intervals.iter().map(|i| i.rem_euclid(12)).collect();
        intervals.sort_unstable();
        intervals.dedup();
        if intervals.is_empty() {
            None
        } else {
            Some(Self {
                intervals: intervals.into_boxed_slice(),
            })
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        let intervals: &[i32] = match name.to_ascii_lowercase().as_str() {
            "major" | "ionian" => &[0, 2, 4, 5, 7, 9, 11],
            "minor" | "aeolian" => &[0, 2, 3, 5, 7, 8, 10],
            "dorian" => &[0, 2, 3, 5, 7, 9, 10],
            "phrygian" => &[0, 1, 3, 5, 7, 8, 10],
            "lydian" => &[0, 2, 4, 6, 7, 9, 11],
            "mixolydian" => &[0, 2, 4, 5, 7, 9, 10],
            "locrian" => &[0, 1, 3, 5, 6, 8, 10],
            "majpent" => &[0, 2, 4, 7, 9],
            "minpent" => &[0, 3, 5, 7, 10],
            "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "whole" => &[0, 2, 4, 6, 8, 10],
            _ => return None,
        };
        Self::new(intervals)
    }

    pub fn parse_degrees(degrees: &str) -> Option<Self> {
        let intervals: Option<Vec<i32>> = degrees
            .split(',')
            .map(|part| part.trim().parse::<i32>().ok())
            .collect();
        Self::new(&intervals?)
    }

    pub fn snap(&self, midi: Sample) -> Sample {
        if !midi.is_finite() {
            return 0.0;
        }

        let base = (midi / 12.0).floor() as i32 * 12;
        let mut best = base + self.intervals[0];
        let mut best_distance = (midi - best as Sample).abs();

        for octave in -1..=1 {
            for &interval in self.intervals.iter() {
                let candidate = base + octave * 12 + interval;
                let distance = (midi - candidate as Sample).abs();
                if distance < best_distance || (distance == best_distance && candidate < best) {
                    best = candidate;
                    best_distance = distance;
                }
            }
        }

        best as Sample
    }
}

impl Op for ScaleQuantizer {
    fn perform(&mut self, stack: &mut Stack) {
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];
        for (out, &midi) in output.iter_mut().zip(&input) {
            *out = self.snap(midi);
        }
        stack.push(&output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_scale_snaps_nearest_with_ties_low() {
        let scale = ScaleQuantizer::named("major").unwrap();
        assert_eq!(scale.snap(61.0), 60.0);
        assert_eq!(scale.snap(61.6), 62.0);
        assert_eq!(scale.snap(71.8), 72.0);
    }

    #[test]
    fn degree_list_matches_named_scale() {
        let named = ScaleQuantizer::named("minor").unwrap();
        let deg = ScaleQuantizer::parse_degrees("0,2,3,5,7,8,10").unwrap();
        for midi in -24..96 {
            assert_eq!(
                deg.snap(midi as Sample + 0.37),
                named.snap(midi as Sample + 0.37)
            );
        }
    }

    #[test]
    fn negative_and_non_finite_inputs_are_handled() {
        let scale = ScaleQuantizer::named("major").unwrap();
        assert_eq!(scale.snap(-0.4), 0.0);
        assert_eq!(scale.snap(-1.2), -1.0);
        assert_eq!(scale.snap(Sample::NAN), 0.0);
    }
}
