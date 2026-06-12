#![allow(deprecated)]

#[macro_use]
extern crate vst;

#[cfg(feature = "allocation-checks")]
use alloc_counter::no_alloc;
use audio_program::{Context, PARAMETERS, compile_program};
use audio_server::Message;
use audio_vm::{AtomicFrame, AtomicSample, CHANNELS, Program, Sample, VM};
use crossbeam_channel::Sender;
use rkyv::{from_bytes, rancor::Error as RkyvError};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::io::Read;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use thread_worker::Worker;
use vst::plugin::{HostCallback, Info, Plugin, PluginParameters};

struct SoundGarden {
    input: Arc<AtomicFrame>,
    params: Arc<Params>,
    server: Worker<ServerInput, ServerOutput>,
    program_rx: Consumer<Program>,
    garbage_tx: Producer<Program>,
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
}

enum ServerOutput {
    Port(u16),
}

impl Default for SoundGarden {
    fn default() -> Self {
        let input = Default::default();
        let input_for_ctx = Arc::clone(&input);
        let (program_tx, program_rx) = RingBuffer::<Program>::new(8);
        let (garbage_tx, mut garbage_rx) = RingBuffer::<Program>::new(8);
        std::thread::spawn(move || {
            loop {
                while let Ok(program) = garbage_rx.pop() {
                    drop(program);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let mut program_tx = program_tx;
        let server = Worker::spawn("TCP Server", 1, move |rx, tx| {
            let mut ctx = Context {
                input: input_for_ctx,
                ..Default::default()
            };
            let sample_rate = Arc::new(AtomicU32::new(48_000));

            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            tx.send(ServerOutput::Port(port)).ok();

            {
                let sample_rate = Arc::clone(&sample_rate);
                let parameters = ctx.params.iter().map(Arc::clone).collect::<Vec<_>>();
                std::thread::spawn(move || {
                    for msg in rx {
                        use ServerInput::*;
                        match msg {
                            Param { index, value } => {
                                parameters[index].store(value.to_bits(), Ordering::Relaxed)
                            }
                            SampleRate(sr) => sample_rate.store(sr, Ordering::Relaxed),
                        }
                    }
                });
            }

            std::thread::spawn(move || {
                for msg in listener.incoming().filter_map(|stream| {
                    stream.ok().and_then(|mut stream| {
                        let mut bytes = Vec::new();
                        stream.read_to_end(&mut bytes).ok()?;
                        from_bytes::<Message, RkyvError>(&bytes).ok()
                    })
                }) {
                    match msg {
                        Message::Play(_x) => {}
                        Message::Record(_x) => {}
                        Message::LoadProgram(ops) => {
                            let program = compile_program(
                                &ops,
                                sample_rate.load(Ordering::Relaxed),
                                &mut ctx,
                            );
                            program_tx.push(program).ok();
                        }
                        Message::Monitor(_) => {}
                        Message::PatternMonitors(_) => {}
                        Message::Oscilloscope(_) => {}
                        Message::Quit => {}
                    }
                }
            });
        });
        let ServerOutput::Port(port) = server.receiver().recv().unwrap();
        let mut vm: VM = Default::default();
        vm.play();
        SoundGarden {
            input,
            params: Arc::new(Params {
                values: Default::default(),
                tx: server.sender().clone(),
                port,
            }),
            server,
            program_rx,
            garbage_tx,
            vm,
        }
    }
}

impl Plugin for SoundGarden {
    fn new(_host: HostCallback) -> Self {
        Self::default()
    }

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

    #[cfg_attr(feature = "allocation-checks", no_alloc)]
    fn process(&mut self, buffer: &mut vst::buffer::AudioBuffer<f32>) {
        audio_vm::enable_flush_to_zero();

        if let Ok(program) = self.program_rx.pop() {
            let garbage = self.vm.load_program(program);
            if let Err(PushError::Full(garbage)) = self.garbage_tx.push(garbage) {
                // Avoid deallocating the old program on the audio thread if the
                // background garbage queue is full.
                std::mem::forget(garbage);
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

    #[cfg_attr(feature = "allocation-checks", no_alloc)]
    fn process_f64(&mut self, buffer: &mut vst::buffer::AudioBuffer<f64>) {
        audio_vm::enable_flush_to_zero();

        if let Ok(program) = self.program_rx.pop() {
            let garbage = self.vm.load_program(program);
            if let Err(PushError::Full(garbage)) = self.garbage_tx.push(garbage) {
                // Avoid deallocating the old program on the audio thread if the
                // background garbage queue is full.
                std::mem::forget(garbage);
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
