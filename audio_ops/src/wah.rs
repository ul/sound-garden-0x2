//! Wah pedal demo from Faust stdlib.
//! Ported semi-automatically: `faust2jackrust wahPedal.dsp`
//! and then extract mydsp.
//!
//! -------------------------`(dm.)wah4_demo`---------------------------
//! Wah pedal application.
//!
//! #### Usage
//!
//! ```
//! _ : wah4_demo : _;
//! ```
//! DSP
//! ```
//! wah4_demo = ba.bypass1(bp, ve.wah4(fr))
//! with{
//! 	wah4_group(x) = hgroup("WAH4 [tooltip: Fourth-order wah effect made using moog_vcf]", x);
//! 	bp = wah4_group(checkbox("[0] Bypass [tooltip: When this is checked, the wah pedal has
//! 		no effect]"));
//! 	fr = wah4_group(hslider("[1] Resonance Frequency [scale:log] [tooltip: wah resonance
//! 		frequency in Hz]", 200,100,2000,1));
//! 	// Avoid dc with the moog_vcf (amplitude too high when freq comes up from dc)
//! 	// Also, avoid very high resonance frequencies (e.g., 5kHz or above).
//! };
//! ```

#![allow(unused_parens)]
#![allow(non_snake_case)]
#![allow(unused_mut)]

use audio_vm::{Op, Stack, CHANNELS};
use itertools::izip;

pub struct WahPedal {
    dsp: DSP,
}

impl WahPedal {
    pub fn new(sample_rate: u32) -> Self {
        let mut dsp = DSP::new();
        dsp.init(sample_rate as _);
        WahPedal { dsp }
    }
}

impl Op for WahPedal {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let freq = stack.pop();
        let gate = stack.pop();
        let input = stack.pop();
        for (&i, o, &gate, &freq) in izip!(&input, &mut frame, &gate, &freq) {
            let inputs = [[i as _], [0.0]];
            let mut outputs = [[0.0], [0.0]];
            self.dsp.fCheckbox0 = if gate > 0.0 { 1.0 } else { 0.0 };
            self.dsp.fHslider0 = freq as _;
            self.dsp.compute(1, &inputs, &mut outputs);
            *o = outputs[0][0] as _;
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.dsp.fRec0 = other.dsp.fRec0;
            self.dsp.fRec1 = other.dsp.fRec1;
            self.dsp.fRec2 = other.dsp.fRec2;
            self.dsp.fRec3 = other.dsp.fRec3;
            self.dsp.fRec4 = other.dsp.fRec4;
            self.dsp.fRec5 = other.dsp.fRec5;
        }
    }
}

struct DSP {
    fCheckbox0: f32,
    fSampleRate: i32,
    fConst0: f32,
    fHslider0: f32,
    fRec5: [f32; 2],
    fRec4: [f32; 2],
    fRec3: [f32; 2],
    fRec2: [f32; 2],
    fRec1: [f32; 2],
    fRec0: [f32; 2],
}

impl DSP {
    fn new() -> Self {
        Self {
            fCheckbox0: 0.0,
            fSampleRate: 0,
            fConst0: 0.0,
            fHslider0: 0.0,
            fRec5: [0.0; 2],
            fRec4: [0.0; 2],
            fRec3: [0.0; 2],
            fRec2: [0.0; 2],
            fRec1: [0.0; 2],
            fRec0: [0.0; 2],
        }
    }

    fn instance_reset_user_interface(&mut self) {
        self.fCheckbox0 = 0.0;
        self.fHslider0 = 200.0;
    }

    fn instance_clear(&mut self) {
        for l0 in 0..2 {
            self.fRec5[l0 as usize] = 0.0;
        }
        for l1 in 0..2 {
            self.fRec4[l1 as usize] = 0.0;
        }
        for l2 in 0..2 {
            self.fRec3[l2 as usize] = 0.0;
        }
        for l3 in 0..2 {
            self.fRec2[l3 as usize] = 0.0;
        }
        for l4 in 0..2 {
            self.fRec1[l4 as usize] = 0.0;
        }
        for l5 in 0..2 {
            self.fRec0[l5 as usize] = 0.0;
        }
    }

    fn instance_constants(&mut self, sample_rate: i32) {
        self.fSampleRate = sample_rate;
        self.fConst0 = 6.28318548 / f32::min(192000.0, f32::max(1.0, self.fSampleRate as f32))
    }

    fn instance_init(&mut self, sample_rate: i32) {
        self.instance_constants(sample_rate);
        self.instance_reset_user_interface();
        self.instance_clear();
    }

    fn init(&mut self, sample_rate: i32) {
        self.instance_init(sample_rate);
    }

    fn compute(&mut self, count: i32, inputs: &[[f32; 1]], outputs: &mut [[f32; 1]]) {
        let mut iSlow0: i32 = ((self.fCheckbox0 as f32) as i32);
        let mut fSlow1: f32 = (0.00100000005 * (self.fHslider0 as f32));
        for i in 0..count {
            let mut fTemp0: f32 = (inputs[0][i as usize] as f32);
            self.fRec5[0] = (fSlow1 + (0.999000013 * self.fRec5[1]));
            let mut fTemp1: f32 = (self.fConst0 * self.fRec5[0]);
            let mut fTemp2: f32 = (1.0 - fTemp1);
            self.fRec4[0] = ((if (iSlow0 as i32 == 1) { 0.0 } else { fTemp0 }
                + (fTemp2 * self.fRec4[1]))
                - (3.20000005 * self.fRec0[1]));
            self.fRec3[0] = (self.fRec4[0] + (fTemp2 * self.fRec3[1]));
            self.fRec2[0] = (self.fRec3[0] + (fTemp2 * self.fRec2[1]));
            self.fRec1[0] = (self.fRec2[0] + (self.fRec1[1] * fTemp2));
            self.fRec0[0] = (self.fRec1[0] * f32::powf(fTemp1, 4.0));
            outputs[0][i as usize] = (if (iSlow0 as i32 == 1) {
                fTemp0
            } else {
                (4.0 * self.fRec0[0])
            } as f32);
            self.fRec5[1] = self.fRec5[0];
            self.fRec4[1] = self.fRec4[0];
            self.fRec3[1] = self.fRec3[0];
            self.fRec2[1] = self.fRec2[0];
            self.fRec1[1] = self.fRec1[0];
            self.fRec0[1] = self.fRec0[0];
        }
    }
}
