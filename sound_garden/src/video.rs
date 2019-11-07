use crate::error::Error;
use crate::logic::Command;
use crate::world::{PlantEditor, PlantEditorMode, Screen, World};
use anyhow::Result;
use crossbeam_channel::Sender;
use lru::LruCache;
use sdl2::{
    pixels::Color,
    rect::{Point, Rect},
    render::{Canvas, Texture, TextureCreator, TextureQuery},
    ttf::Font,
    video::{Window, WindowContext},
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
const CHAR_SIZE: u16 = 24;
const CHAR_TEXTURE_CACHE: usize = 256;

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

    let texture_creator = canvas.texture_creator();
    let mut texture_cache = LruCache::new(CHAR_TEXTURE_CACHE);

    world.lock().unwrap().cell_size = main_fnt.size_of_char('M')?;

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

        render_world(
            &mut canvas,
            &world.lock().unwrap(),
            &texture_creator,
            &mut texture_cache,
            &main_fnt,
        )?;

        if let Some(budget) = frame_budget(frame_start) {
            std::thread::sleep(budget);
        }
    }
}

fn render_world<'a>(
    canvas: &mut Canvas<Window>,
    world: &World,
    texture_creator: &'a TextureCreator<WindowContext>,
    texture_cache: &mut LruCache<(char, Color), Texture<'a>>,
    fnt: &Font,
) -> Result<()> {
    canvas.set_draw_color(Color::RGB(255, 255, 255));
    canvas.clear();

    // Update & draw stuff.
    let cell_size = world.cell_size;
    let anima_color = Color::RGB(0x44, 0x33, 0x55);
    match &world.screen {
        Screen::Garden => {
            for p in &world.plants {
                render_char(
                    canvas,
                    p.symbol,
                    Color::RGB(0, 0, 0),
                    Point::new(p.position.x, p.position.y),
                    cell_size,
                    texture_creator,
                    texture_cache,
                    fnt,
                )?;
            }
            let p = &world.garden.anima_position;
            render_char(
                canvas,
                '@',
                anima_color,
                Point::new(p.x, p.y),
                cell_size,
                texture_creator,
                texture_cache,
                fnt,
            )?;
        }
        Screen::Plant(PlantEditor {
            ix,
            cursor_position,
            mode,
        }) => {
            let p = &world.plants[*ix];
            for node in &p.nodes {
                let p = &node.position;
                render_str(
                    canvas,
                    &node.op,
                    Color::RGB(0, 0, 0),
                    Point::new(p.x, p.y),
                    cell_size,
                    texture_creator,
                    texture_cache,
                    fnt,
                )?;
            }
            canvas.set_draw_color(Color::RGB(0, 0, 0));
            for (i, j) in &p.edges {
                let n1 = &p.nodes[*i];
                let n2 = &p.nodes[*j];
                canvas
                    .draw_line(
                        Point::from((
                            n1.position.x * (cell_size.0 as i32) + (cell_size.0 as i32) / 2,
                            ((n1.position.y + 1) * (cell_size.1 as i32)) - 1,
                        )),
                        Point::from((
                            n2.position.x * (cell_size.0 as i32) + (cell_size.0 as i32) / 2,
                            (n2.position.y * (cell_size.1 as i32)) - 2,
                        )),
                    )
                    .map_err(|s| Error::Draw(s))?;
                canvas
                    .draw_point(Point::from((
                        n2.position.x * (cell_size.0 as i32) + (cell_size.0 as i32) / 2,
                        (n2.position.y * (cell_size.1 as i32)) - 1,
                    )))
                    .map_err(|s| Error::Draw(s))?;
            }
            let p = cursor_position;
            let c = match mode {
                PlantEditorMode::Normal => '@',
                PlantEditorMode::Insert => '_',
                PlantEditorMode::Move(_) => '/',
            };
            render_char(
                canvas,
                c,
                anima_color,
                Point::new(p.x, p.y),
                cell_size,
                texture_creator,
                texture_cache,
                fnt,
            )?;
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

fn render_char<'a>(
    canvas: &mut Canvas<Window>,
    ch: char,
    color: Color,
    topleft: Point,
    cell_size: (u32, u32),
    texture_creator: &'a TextureCreator<WindowContext>,
    texture_cache: &mut LruCache<(char, Color), Texture<'a>>,
    fnt: &Font,
) -> Result<()> {
    let texture = {
        if let Some(texture) = texture_cache.get(&(ch, color)) {
            texture
        } else {
            let surface = fnt.render_char(ch).blended(color)?;
            let texture = texture_creator.create_texture_from_surface(surface)?;
            texture_cache.put((ch, color), texture);
            texture_cache.get(&(ch, color)).unwrap()
        }
    };
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

fn render_str<'a>(
    canvas: &mut Canvas<Window>,
    s: &str,
    color: Color,
    topleft: Point,
    cell_size: (u32, u32),
    texture_creator: &'a TextureCreator<WindowContext>,
    texture_cache: &mut LruCache<(char, Color), Texture<'a>>,
    fnt: &Font,
) -> Result<()> {
    let mut topleft = topleft.clone();
    for c in s.chars() {
        render_char(
            canvas,
            c,
            color,
            topleft,
            cell_size,
            texture_creator,
            texture_cache,
            fnt,
        )?;
        topleft.x += 1;
    }
    Ok(())
}
