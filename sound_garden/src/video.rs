use crate::error::Error;
use crate::logic::{Command, Direction};
use crate::world::World;
use anyhow::Result;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use sdl2::{
    event::Event,
    keyboard::Keycode,
    pixels::Color,
    rect::{Point, Rect},
    render::{Canvas, TextureQuery},
    ttf::Font,
    video::Window,
    EventPump,
};
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 800;
const TITLE: &str = "Sound Garden";
const TARGET_FPS: u32 = 60;
const TARGET_FRAME_DURATION_NS: u32 = 1_000_000_000u32 / TARGET_FPS;
const REGULAR_FONT: &str = "dat/fnt/IBMPlexMono-Regular.ttf";
const CHAR_SIZE: u16 = 16;

pub fn main(rx: Receiver<World>, tx: Sender<Command>) -> Result<()> {
    let sdl_ctx = sdl2::init().map_err(|s| Error::SDLInit(s))?;
    let window = sdl_ctx
        .video()
        .map_err(|s| Error::Video(s))?
        .window(TITLE, WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .opengl()
        .build()?;
    let mut canvas = window.into_canvas().build()?;
    let mut event_pump = sdl_ctx.event_pump().map_err(|s| Error::EventPump(s))?;
    let ttf_ctx = sdl2::ttf::init()?;

    let main_fnt = ttf_ctx
        .load_font(REGULAR_FONT, CHAR_SIZE)
        .map_err(|s| Error::LoadFont(s))?;

    // Start with a blank canvas.
    canvas.set_draw_color(Color::RGB(255, 255, 255));
    canvas.clear();
    canvas.present();

    let target_frame_duration = Duration::new(0, TARGET_FRAME_DURATION_NS);
    let frame_budget = |frame_start: Instant| {
        let frame_duration = frame_start.elapsed();
        if frame_duration < target_frame_duration {
            Some(target_frame_duration - frame_duration)
        } else {
            None
        }
    };

    loop {
        let frame_start = Instant::now();

        process_events(&mut event_pump, &tx)?;

        while let Some(budget) = frame_budget(frame_start) {
            match rx.recv_timeout(budget) {
                Ok(world) => render_world(&mut canvas, &main_fnt, &world)?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

fn render_world(canvas: &mut Canvas<Window>, main_fnt: &Font, world: &World) -> Result<()> {
    canvas.set_draw_color(Color::RGB(255, 255, 255));
    canvas.clear();

    // Update & draw stuff.
    render_char(canvas, &main_fnt, '@', world.anima.position)?;

    // Flip!
    canvas.present();
    Ok(())
}

fn process_events(event_pump: &mut EventPump, tx: &Sender<Command>) -> Result<()> {
    for event in event_pump.poll_iter() {
        match event {
            Event::Quit { .. }
            | Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => tx.send(Command::Quit)?,
            Event::KeyDown {
                keycode: Some(Keycode::Left),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::H),
                ..
            } => tx.send(Command::Move(Direction::Left))?,
            Event::KeyDown {
                keycode: Some(Keycode::Right),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::L),
                ..
            } => tx.send(Command::Move(Direction::Right))?,
            Event::KeyDown {
                keycode: Some(Keycode::Up),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::K),
                ..
            } => tx.send(Command::Move(Direction::Up))?,
            Event::KeyDown {
                keycode: Some(Keycode::Down),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::J),
                ..
            } => tx.send(Command::Move(Direction::Down))?,
            _ => {}
        }
    }
    Ok(())
}

fn render_char(canvas: &mut Canvas<Window>, fnt: &Font, ch: char, topleft: Point) -> Result<()> {
    let surface = fnt.render_char(ch).solid(Color::RGB(0, 0, 0))?;
    let texture_creator = canvas.texture_creator();
    let texture = texture_creator.create_texture_from_surface(surface)?;
    let TextureQuery { width, height, .. } = texture.query();
    canvas
        .copy(
            &texture,
            None,
            Some(Rect::new(
                topleft.x * (width as i32),
                topleft.y * (height as i32),
                width,
                height,
            )),
        )
        .map_err(|s| Error::TextureCopy(s))?;
    Ok(())
}
