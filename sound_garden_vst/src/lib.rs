#[macro_use]
extern crate vst;

use alloc_counter::no_alloc;
use audio_program::{compile_program, Context, TextOp, PARAMETERS};
use audio_vm::{AtomicFrame, AtomicSample, Program, Sample, CHANNELS, VM};
use crossbeam_channel::Sender;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use thread_worker::Worker;
use vst::plugin::{Info, Plugin, PluginParameters};

struct SoundGarden {
    input: Arc<AtomicFrame>,
    params: Arc<Params>,
    server: Worker<ServerInput, ServerOutput>,
    vm: VM,
}

struct Params {
    values: [AtomicSample; PARAMETERS],
    tx: Sender<ServerInput>,
    port: u16,
}

enum ServerInput {
    Param { index: usize, value: Sample },
    SampleRate(u32),
    Garbage(Program),
}

enum ServerOutput {
    Port(u16),
    Program(Program),
}

impl Default for SoundGarden {
    fn default() -> Self {
        let input = Default::default();
        let input_for_ctx = Arc::clone(&input);
        let server = Worker::spawn("TCP Server", 1, move |rx, tx| {
            let mut ctx: Context = Default::default();
            ctx.input = input_for_ctx;
            let sample_rate = Arc::new(AtomicU32::new(48_000));

            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            tx.send(ServerOutput::Port(port)).ok();

            {
                let sample_rate = Arc::clone(&sample_rate);
                let parameters = ctx.params.iter().map(|x| Arc::clone(x)).collect::<Vec<_>>();
                std::thread::spawn(move || {
                    for msg in rx {
                        use ServerInput::*;
                        match msg {
                            Param { index, value } => {
                                parameters[index].store(value.to_bits(), Ordering::Relaxed)
                            }
                            SampleRate(sr) => sample_rate.store(sr, Ordering::Relaxed),
                            Garbage(program) => drop(program),
                        }
                    }
                });
            }

            std::thread::spawn(move || {
                for ops in listener.incoming().filter_map(|stream| {
                    stream
                        .ok()
                        .and_then(|stream| serde_json::from_reader::<_, Vec<TextOp>>(stream).ok())
                }) {
                    let program =
                        compile_program(&ops, sample_rate.load(Ordering::Relaxed), &mut ctx);
                    tx.send(ServerOutput::Program(program)).ok();
                }
            });
        });
        let mut port = 0;
        if let ServerOutput::Port(p) = server.receiver().recv().unwrap() {
            port = p;
        };
        SoundGarden {
            input,
            params: Arc::new(Params {
                values: Default::default(),
                tx: server.sender().clone(),
                port,
            }),
            server,
            vm: Default::default(),
        }
    }
}

impl Plugin for SoundGarden {
    fn get_info(&self) -> Info {
        Info {
            name: "Sound Garden".to_string(),
            vendor: "Ruslan Prokopchuk".to_string(),
            unique_id: 1_804_198_801,
            inputs: CHANNELS as _,
            outputs: CHANNELS as _,
            f64_precision: true,
            parameters: (PARAMETERS + 1) as _, // param:<N> + port
            version: 1,
            category: vst::plugin::Category::Synth,
            ..Default::default()
        }
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.server
            .sender()
            .send(ServerInput::SampleRate(sample_rate as _))
            .ok();
    }

    #[no_alloc]
    fn process(&mut self, buffer: &mut vst::buffer::AudioBuffer<f32>) {
        if let Ok(msg) = self.server.receiver().try_recv() {
            if let ServerOutput::Program(program) = msg {
                let garbage = self.vm.load_program(program);
                self.server
                    .sender()
                    .send(ServerInput::Garbage(garbage))
                    .ok();
            }
        }

        let (inputs, outputs) = buffer.split();

        // Iterate over inputs as (&f32, &f32)
        let (left, right) = inputs.split_at(1);
        let stereo_in = left[0].iter().zip(right[0].iter());

        // Iterate over outputs as (&mut f32, &mut f32)
        let (mut left, mut right) = outputs.split_at_mut(1);
        let stereo_out = left[0].iter_mut().zip(right[0].iter_mut());

        // Zip and process
        for ((left_in, right_in), (left_out, right_out)) in stereo_in.zip(stereo_out) {
            self.input[0].store(Sample::from(*left_in).to_bits(), Ordering::Relaxed);
            self.input[1].store(Sample::from(*right_in).to_bits(), Ordering::Relaxed);
            let output = self.vm.next_frame();
            *left_out = output[0] as f32;
            *right_out = output[1] as f32;
        }
    }

    #[no_alloc]
    fn process_f64(&mut self, buffer: &mut vst::buffer::AudioBuffer<f64>) {
        if let Ok(msg) = self.server.receiver().try_recv() {
            if let ServerOutput::Program(program) = msg {
                let garbage = self.vm.load_program(program);
                self.server
                    .sender()
                    .send(ServerInput::Garbage(garbage))
                    .ok();
            }
        }

        let (inputs, outputs) = buffer.split();

        // Iterate over inputs as (&f32, &f32)
        let (left, right) = inputs.split_at(1);
        let stereo_in = left[0].iter().zip(right[0].iter());

        // Iterate over outputs as (&mut f32, &mut f32)
        let (mut left, mut right) = outputs.split_at_mut(1);
        let stereo_out = left[0].iter_mut().zip(right[0].iter_mut());

        // Zip and process
        for ((left_in, right_in), (left_out, right_out)) in stereo_in.zip(stereo_out) {
            self.input[0].store(left_in.to_bits(), Ordering::Relaxed);
            self.input[1].store(right_in.to_bits(), Ordering::Relaxed);
            let output = self.vm.next_frame();
            *left_out = output[0];
            *right_out = output[1];
        }
    }
}

impl PluginParameters for Params {
    fn get_parameter(&self, index: i32) -> f32 {
        if index < PARAMETERS as _ {
            f64::from_bits(self.values[index as usize].load(Ordering::Relaxed)) as _
        } else {
            0.0
        }
    }

    fn set_parameter(&self, index: i32, value: f32) {
        let index = index as usize;
        if index < PARAMETERS {
            let value = Sample::from(value);
            self.values[index].store(value.to_bits(), Ordering::Relaxed);
            self.tx.send(ServerInput::Param { index, value }).ok();
        }
    }

    fn get_parameter_name(&self, index: i32) -> String {
        if index < PARAMETERS as _ {
            format!("param:{}", index)
        } else if index == PARAMETERS as i32 {
            String::from("PORT (readonly)")
        } else {
            String::new()
        }
    }

    fn get_parameter_text(&self, index: i32) -> String {
        if index < PARAMETERS as _ {
            format!(
                "{}",
                f64::from_bits(self.values[index as usize].load(Ordering::Relaxed))
            )
        } else if index == PARAMETERS as i32 {
            format!("{}", self.port)
        } else {
            String::new()
        }
    }

    fn can_be_automated(&self, _index: i32) -> bool {
        true
    }
}

plugin_main!(SoundGarden); // Important!
