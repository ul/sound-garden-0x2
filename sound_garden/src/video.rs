use crate::error::Error;
use crate::logic::Command;
use crate::world::{PlantEditor, Screen, World};
use anyhow::Result;
use crossbeam_channel::Sender;
use sdl2::{
    gfx::primitives::DrawRenderer,
    pixels::Color,
    rect::{Point, Rect},
    render::{Canvas, TextureQuery},
    ttf::Font,
    video::Window,
    EventPump,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 800;
const TITLE: &str = "Sound Garden";
const TARGET_FPS: u32 = 60;
const TARGET_FRAME_DURATION_NS: u32 = 1_000_000_000u32 / TARGET_FPS;
const REGULAR_FONT: &str = "dat/fnt/Agave-Regular.ttf";
const CHAR_SIZE: u16 = 16;
const GOLD_CHAR: char = '@';

pub fn main(world: Arc<Mutex<World>>, tx: Sender<Command>) -> Result<()> {
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

    world.lock().unwrap().cell_size = main_fnt.size_of_char(GOLD_CHAR)?;

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

        render_world(&mut canvas, &main_fnt, &world.lock().unwrap())?;

        if let Some(budget) = frame_budget(frame_start) {
            std::thread::sleep(budget);
        }
    }
}

fn render_world(canvas: &mut Canvas<Window>, main_fnt: &Font, world: &World) -> Result<()> {
    canvas.set_draw_color(Color::RGB(255, 255, 255));
    canvas.clear();

    // Update & draw stuff.
    let cell_size = world.cell_size;
    match world.screen {
        Screen::Garden => {
            for p in &world.plants {
                render_char(canvas, &main_fnt, p.symbol, p.position, cell_size)?;
            }
            render_char(
                canvas,
                &main_fnt,
                '@',
                world.garden.anima_position,
                cell_size,
            )?;
        }
        Screen::Plant(PlantEditor {
            ix,
            cursor_position,
            ..
        }) => {
            let p = &world.plants[ix];
            for node in &p.nodes {
                render_str(canvas, &main_fnt, &node.op, node.position, cell_size)?;
            }
            for (i, j) in &p.edges {
                let n1 = &p.nodes[*i];
                let n2 = &p.nodes[*j];
                canvas
                    .line(
                        (n1.position.x as i16) * (cell_size.0 as i16) + (cell_size.0 as i16) / 2,
                        (n1.position.y as i16 + 1) * (cell_size.1 as i16),
                        (n2.position.x as i16) * (cell_size.0 as i16) + (cell_size.0 as i16) / 2,
                        (n2.position.y as i16) * (cell_size.1 as i16),
                        Color::from((0, 0, 0)),
                    )
                    .map_err(|s| Error::Draw(s))?;
            }
            render_char(canvas, &main_fnt, '_', cursor_position, cell_size)?;
        }
    }

    // Flip!
    canvas.present();
    Ok(())
}

fn process_events(event_pump: &mut EventPump, tx: &Sender<Command>) -> Result<()> {
    for event in event_pump.poll_iter() {
        tx.send(Command::SDLEvent(event))?;
    }
    Ok(())
}

fn render_char(
    canvas: &mut Canvas<Window>,
    fnt: &Font,
    ch: char,
    topleft: Point,
    cell_size: (u32, u32),
) -> Result<()> {
    let surface = fnt.render_char(ch).solid(Color::RGB(0, 0, 0))?;
    let texture_creator = canvas.texture_creator();
    let texture = texture_creator.create_texture_from_surface(surface)?;
    let TextureQuery { width, height, .. } = texture.query();
    canvas
        .copy(
            &texture,
            None,
            Some(Rect::new(
                topleft.x * (cell_size.0 as i32),
                topleft.y * (cell_size.1 as i32),
                width,
                height,
            )),
        )
        .map_err(|s| Error::TextureCopy(s))?;
    Ok(())
}

fn render_str(
    canvas: &mut Canvas<Window>,
    fnt: &Font,
    s: &str,
    topleft: Point,
    cell_size: (u32, u32),
) -> Result<()> {
    let surface = fnt.render(s).solid(Color::RGB(0, 0, 0))?;
    let texture_creator = canvas.texture_creator();
    let texture = texture_creator.create_texture_from_surface(surface)?;
    let TextureQuery { width, height, .. } = texture.query();
    canvas
        .copy(
            &texture,
            None,
            Some(Rect::new(
                topleft.x * (cell_size.0 as i32),
                topleft.y * (cell_size.1 as i32),
                width,
                height,
            )),
        )
        .map_err(|s| Error::TextureCopy(s))?;
    Ok(())
}
