use crossbeam_channel::select;
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use termion::event::Key;
use termion::input::TermRead;
use thread_worker::Worker;

pub enum Event<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events.
pub struct Events {
    input_worker: Worker<(), Event<Key>>,
    ignore_exit_key: Arc<AtomicBool>,
    tick_rate: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub exit_key: Key,
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            exit_key: Key::Char('q'),
            tick_rate: Duration::from_millis(250),
        }
    }
}

const CHANNEL_CAPACITY: usize = 1024;

impl Events {
    pub fn new() -> Events {
        Events::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Events {
        let ignore_exit_key = Arc::new(AtomicBool::new(false));
        let input_worker = {
            let ignore_exit_key = Arc::clone(&ignore_exit_key);
            Worker::spawn("Input", CHANNEL_CAPACITY, move |_, tx| {
                for key in io::stdin().keys().filter_map(|k| k.ok()) {
                    if tx.send(Event::Input(key)).is_err() {
                        return;
                    }
                    if !ignore_exit_key.load(Ordering::Relaxed) && key == config.exit_key {
                        return;
                    }
                }
            })
        };
        Events {
            ignore_exit_key,
            input_worker,
            tick_rate: config.tick_rate,
        }
    }

    pub fn next(&self) -> Result<Event<Key>, crossbeam_channel::RecvError> {
        select! {
            recv(self.input_worker.receiver()) -> msg => msg,
            default(self.tick_rate) => Ok(Event::Tick)
        }
    }

    pub fn disable_exit_key(&mut self) {
        self.ignore_exit_key.store(true, Ordering::Relaxed);
    }

    pub fn enable_exit_key(&mut self) {
        self.ignore_exit_key.store(false, Ordering::Relaxed);
    }
}
