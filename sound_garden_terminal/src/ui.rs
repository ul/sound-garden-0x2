use crate::event::{Event, Events};
use anyhow::{anyhow, Result};
use audio_program::{compile_program, get_help, rewrite_terms, Context, TextOp};
use audio_vm::VM;
use chrono::prelude::*;
use crossbeam_channel::Sender;
use itertools::Itertools;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use termion::cursor;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::Rect;
use tui::style::{Color, Style};
use tui::widgets::{Block, Borders, Paragraph, Text, Widget};
use tui::Terminal;

pub fn main(
    vm: Arc<Mutex<VM>>,
    sample_rate: u32,
    filename: &str,
    record_tx: &Sender<bool>,
) -> Result<()> {
    let mut app = App::load(&filename).unwrap_or_else(|_| App::new());
    commit(&mut app, Arc::clone(&vm), sample_rate, filename);
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut events = Events::new();
    loop {
        app.status = String::new();
        if let Some(ix) = app.node_at_cursor() {
            let node = &app.nodes[ix];
            if app.cursor.x < node.position.x + node.op.len() {
                if let Some(help) = app.op_help.get(&node.op) {
                    app.status = help.to_owned();
                }
            }
        }
        match app.screen {
            Screen::Editor => render_editor(&mut app, &mut terminal)?,
            Screen::Help => render_help(&mut app, sample_rate, &filename, &mut terminal)?,
        };

        match app.screen {
            Screen::Editor => handle_editor(
                &mut app,
                Arc::clone(&vm),
                sample_rate,
                &filename,
                &mut events,
                record_tx,
            )?,
            Screen::Help => handle_help(&mut app, &mut events)?,
        };
    }
}

fn render_editor(
    app: &mut App,
    terminal: &mut Terminal<
        TermionBackend<AlternateScreen<MouseTerminal<termion::raw::RawTerminal<std::io::Stdout>>>>,
    >,
) -> Result<()> {
    terminal.draw(|mut f| {
        let size = f.size();
        let mut nodes_to_drop = Vec::new();
        for (
            i,
            Node {
                op,
                draft,
                position: p,
                ..
            },
        ) in app.nodes.iter().enumerate()
        {
            if p.x < 2 || p.y < 2 || p.x + op.len() > size.width as _ || p.y + 1 > size.height as _
            {
                nodes_to_drop.push(i);
                continue;
            }
            let text = [Text::raw(op.to_owned())];
            Paragraph::new(text.iter())
                .style(Style::default().fg(if *draft { Color::Red } else { Color::White }))
                .render(
                    &mut f,
                    Rect::new((p.x - 1) as _, (p.y - 1) as _, op.len() as _, 1),
                );
        }
        for ix in nodes_to_drop.drain(..) {
            app.nodes.swap_remove(ix);
            app.draft = true;
        }
        let color = if !app.play {
            Color::Gray
        } else if app.draft || app.nodes.iter().any(|node| node.draft) {
            Color::Red
        } else {
            Color::White
        };
        Block::default()
            .title(&format!(
                "Sound Garden────{}────{}────{}",
                if app.play { "|>" } else { "||" },
                if app.recording {
                    if Utc::now().second() % 2 == 0 {
                        "•R"
                    } else {
                        " R"
                    }
                } else {
                    ""
                },
                app.status
            ))
            .title_style(Style::default().fg(color))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .render(&mut f, size);
    })?;
    write!(
        terminal.backend_mut(),
        "{}{}",
        cursor::Show,
        cursor::Goto(app.cursor.x as _, app.cursor.y as _),
    )?;
    match app.input_mode {
        InputMode::Normal => write!(terminal.backend_mut(), "{}", cursor::SteadyBlock)?,
        InputMode::Editing => write!(terminal.backend_mut(), "{}", cursor::SteadyUnderline)?,
    }
    io::stdout().flush()?;
    Ok(())
}

fn render_help(
    app: &mut App,
    sample_rate: u32,
    filename: &str,
    terminal: &mut Terminal<
        TermionBackend<AlternateScreen<MouseTerminal<termion::raw::RawTerminal<std::io::Stdout>>>>,
    >,
) -> Result<()> {
    terminal.draw(|mut f| {
        let mut size = f.size();
        Block::default()
            .title("Sound Garden────Help")
            .title_style(Style::default().fg(Color::Green))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .render(&mut f, size);
        let text = [
            Text::raw(format!("Path: {}\n", filename)),
            Text::raw(format!("Sample rate: {}\n", sample_rate)),
            Text::raw(format!(
                "Cycles: {}\n",
                app.cycles.iter().map(|cycle| cycle.join("->")).join(", ")
            )),
            Text::raw(format!("Program: {}\n", app.program)),
            Text::raw(format!("\n")),
            Text::raw(include_str!("help.txt")),
        ];
        size.x = 2;
        size.y = 2;
        size.width -= 3;
        size.height -= 3;
        Paragraph::new(text.iter())
            .scroll(app.help_scroll)
            .wrap(true)
            .render(&mut f, size);
    })?;
    write!(terminal.backend_mut(), "{}", cursor::Hide,)?;
    io::stdout().flush()?;
    Ok(())
}

fn handle_editor(
    app: &mut App,
    vm: Arc<Mutex<VM>>,
    sample_rate: u32,
    filename: &str,
    events: &mut Events,
    record_tx: &Sender<bool>,
) -> Result<()> {
    match events.next()? {
        Event::Input(input) => match app.input_mode {
            InputMode::Normal => match input {
                Key::Char('\n') => commit(app, vm, sample_rate, filename),
                Key::Char('\\') => {
                    app.play = !app.play;
                    if app.play {
                        vm.lock().unwrap().play();
                    } else {
                        vm.lock().unwrap().pause();
                    }
                }
                Key::Char('i') => {
                    app.input_mode = InputMode::Editing;
                    events.disable_exit_key();
                }
                Key::Char('I') => {
                    app.input_mode = InputMode::Editing;
                    events.disable_exit_key();
                    let mut push_left = 1;
                    let push_right = 1;
                    if let Some(ix) = app.node_at_cursor() {
                        let Node {
                            op, position: p, ..
                        } = &app.nodes[ix];
                        push_left += p.x + op.len() - app.cursor.x;
                    }
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                    {
                        node.position.x -= push_left;
                    }
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && node.position.x > p.x)
                    {
                        node.position.x += push_right;
                    }
                }
                Key::Char('c') => {
                    app.input_mode = InputMode::Editing;
                    events.disable_exit_key();
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes[ix];
                        let push_left = node.position.x + node.op.len() - app.cursor.x;
                        node.op.truncate(app.cursor.x - node.position.x);
                        node.draft = true;
                        let p = app.cursor;
                        for node in app
                            .nodes
                            .iter_mut()
                            .filter(|node| node.position.y == p.y && node.position.x > p.x)
                        {
                            node.position.x -= push_left;
                        }
                    }
                }
                Key::Char('h') | Key::Left | Key::Backspace => {
                    app.cursor.x -= 1;
                }
                Key::Char('j') | Key::Down => {
                    app.cursor.y += 1;
                }
                Key::Char('k') | Key::Up => {
                    app.cursor.y -= 1;
                }
                Key::Char('l') | Key::Right | Key::Char(' ') => {
                    app.cursor.x += 1;
                }
                Key::Alt('h') => {
                    if let Some(ix) = app.node_at_cursor() {
                        app.nodes[ix].position.x -= 1;
                    }
                    app.cursor.x -= 1;
                }
                Key::Alt('j') => {
                    if let Some(ix) = app.node_at_cursor() {
                        app.nodes[ix].position.y += 1;
                    }
                    app.cursor.y += 1;
                }
                Key::Alt('k') => {
                    if let Some(ix) = app.node_at_cursor() {
                        app.nodes[ix].position.y -= 1;
                    }
                    app.cursor.y -= 1;
                }
                Key::Alt('l') => {
                    if let Some(ix) = app.node_at_cursor() {
                        app.nodes[ix].position.x += 1;
                    }
                    app.cursor.x += 1;
                }
                Key::Char('J') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| {
                        node.position.y > p.y
                            || node.position.y == p.y && p.x < node.position.x + node.op.len()
                    }) {
                        node.position.y += 1;
                    }
                    app.cursor.y += 1;
                }
                Key::Char('K') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| {
                        node.position.y < p.y || node.position.y == p.y && node.position.x <= p.x
                    }) {
                        node.position.y -= 1;
                    }
                    app.cursor.y -= 1;
                }
                Key::Char('H') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| node.position.y == p.y) {
                        node.position.y -= 1;
                    }
                    app.cursor.y -= 1;
                }
                Key::Char('L') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| node.position.y == p.y) {
                        node.position.y += 1;
                    }
                    app.cursor.y += 1;
                }
                Key::Char(',') => {
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                    {
                        node.position.x -= 1;
                    }
                    app.cursor.x -= 1;
                }
                Key::Char('<') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| {
                        node.position.y == p.y && p.x < node.position.x + node.op.len()
                    }) {
                        node.position.x -= 1;
                    }
                    app.cursor.x -= 1;
                }
                Key::Char('.') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| {
                        node.position.y == p.y && p.x < node.position.x + node.op.len()
                    }) {
                        node.position.x += 1;
                    }
                    app.cursor.x += 1;
                }
                Key::Char('>') => {
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                    {
                        node.position.x += 1;
                    }
                    app.cursor.x += 1;
                }
                Key::Char('d') => {
                    if let Some(ix) = app.node_at_cursor() {
                        app.nodes.swap_remove(ix);
                        app.draft = true;
                    }
                }
                Key::Char('D') => {
                    let p = app.cursor;
                    app.nodes.retain(|node| node.position.y != p.y);
                }
                Key::Char('=') => {
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes[ix];
                        let i = app.cursor.x - node.position.x;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 1) % 10;
                            node.op.replace_range(i..(i + 1), &d.to_string());

                            commit(app, vm, sample_rate, filename);
                        } else {
                            for cycle in &app.cycles {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[0] == node.op) {
                                    node.op = ops[1].to_owned();
                                    commit(app, vm, sample_rate, filename);
                                    break;
                                }
                            }
                        }
                    }
                }
                Key::Char('-') => {
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes[ix];
                        let i = app.cursor.x - node.position.x;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 9) % 10;
                            node.op.replace_range(i..(i + 1), &d.to_string());
                            commit(app, vm, sample_rate, filename);
                        } else {
                            for cycle in &app.cycles {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[1] == node.op) {
                                    node.op = ops[0].to_owned();
                                    commit(app, vm, sample_rate, filename);
                                    break;
                                }
                            }
                        }
                    }
                }
                Key::Char('r') => {
                    app.recording = !app.recording;
                    record_tx.send(app.recording).ok();
                }
                Key::Char('q') => {
                    vm.lock().unwrap().pause();
                    return Err(anyhow!("Quit!"));
                }
                Key::Char('?') => app.screen = Screen::Help,
                _ => {}
            },
            InputMode::Editing => match input {
                Key::Left => {
                    app.cursor.x -= 1;
                }
                Key::Down => {
                    app.cursor.y += 1;
                }
                Key::Up => {
                    app.cursor.y -= 1;
                }
                Key::Right => {
                    app.cursor.x += 1;
                }
                Key::Char(' ') => {
                    let p = app.cursor;
                    for node in app.nodes.iter_mut().filter(|node| {
                        node.position.y == p.y && p.x < node.position.x + node.op.len()
                    }) {
                        node.position.x += 1;
                    }
                    app.cursor.x += 1;
                }
                Key::Char('\n') => {
                    app.input_mode = InputMode::Normal;
                    events.enable_exit_key();
                    app.draft = app.nodes.iter().any(|node| node.op.is_empty());
                    app.nodes.retain(|node| !node.op.is_empty());
                }
                Key::Char(c) => {
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && p.x < node.position.x)
                    {
                        node.position.x += 1;
                    }
                    let node = app.node_at_cursor();
                    if let Some(ix) = node {
                        let node = &mut app.nodes[ix];
                        if app.cursor.x >= node.position.x + node.op.chars().count() {
                            node.op.push(c);
                        } else {
                            let ix = node
                                .op
                                .char_indices()
                                .nth((app.cursor.x - node.position.x) as usize)
                                .map(|x| x.0)
                                .unwrap();
                            node.op.insert(ix, c);
                        }
                        node.draft = true;
                    } else {
                        let node = Node {
                            id: random(),
                            draft: true,
                            op: c.to_string(),
                            position: app.cursor,
                        };
                        app.nodes.push(node);
                    };
                    app.cursor.x += 1;
                }
                Key::Backspace => {
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && p.x < node.position.x)
                    {
                        node.position.x -= 1;
                    }
                    app.cursor.x -= 1;
                    let node = app.node_at_cursor();
                    if let Some(ix) = node {
                        let node = &mut app.nodes[ix];
                        if app.cursor.x < node.position.x + node.op.len() {
                            if node.op.len() > 1 {
                                let x = (app.cursor.x - node.position.x) as usize;
                                let ixs = node
                                    .op
                                    .char_indices()
                                    .skip(x)
                                    .take(2)
                                    .map(|x| x.0)
                                    .collect::<Vec<_>>();
                                node.op.replace_range(
                                    ixs[0]..*(ixs.get(1).unwrap_or(&node.op.len())),
                                    &"",
                                );
                            } else if app.cursor.x == node.position.x {
                                node.op.pop();
                            };
                            node.draft = true;
                        }
                    }
                }
                Key::Esc => {
                    app.input_mode = InputMode::Normal;
                    events.enable_exit_key();
                    app.draft = app.nodes.iter().any(|node| node.op.is_empty());
                    app.nodes.retain(|node| !node.op.is_empty());
                }
                _ => {}
            },
        },
        _ => {}
    }
    Ok(())
}

fn handle_help(app: &mut App, events: &mut Events) -> Result<()> {
    match events.next()? {
        Event::Input(input) => match input {
            Key::Char('?') => app.screen = Screen::Editor,
            Key::Esc => app.screen = Screen::Editor,
            Key::Char('j') | Key::Down => {
                app.help_scroll += 1;
            }
            Key::Char('k') | Key::Up => {
                if app.help_scroll > 0 {
                    app.help_scroll -= 1;
                }
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

fn commit(app: &mut App, vm: Arc<Mutex<VM>>, sample_rate: u32, filename: &str) {
    app.nodes.sort_by_key(|node| node.position);
    app.program = app.nodes.iter().map(|node| node.op.to_owned()).join(" ");
    app.nodes.iter_mut().for_each(|node| node.draft = false);
    app.draft = false;
    let next_ops = rewrite_terms(
        &app.nodes
            .iter()
            .map(|Node { id, op, .. }| TextOp {
                id: *id,
                op: op.to_owned(),
            })
            .collect::<Vec<_>>(),
    );
    if app.ops != next_ops {
        app.ops = next_ops;
        app.save(&filename).ok();
        let new_program = compile_program(&app.ops, sample_rate, &mut app.ctx);
        // Ensure the smallest possible scope to limit locking time.
        let garbage = {
            vm.lock().unwrap().load_program(new_program);
        };
        drop(garbage);
    }
}

#[derive(Serialize, Deserialize)]
struct App {
    #[serde(skip, default)]
    ctx: Context,
    cursor: Position,
    #[serde(skip, default = "default_cycles")]
    cycles: Vec<Vec<String>>,
    #[serde(skip, default)]
    draft: bool,
    #[serde(skip, default)]
    help_scroll: u16,
    #[serde(skip, default)]
    input_mode: InputMode,
    nodes: Vec<Node>,
    #[serde(skip, default = "get_help")]
    op_help: HashMap<String, String>,
    #[serde(skip, default)]
    ops: Vec<TextOp>,
    #[serde(skip, default)]
    play: bool,
    #[serde(default)]
    program: String,
    #[serde(skip, default)]
    recording: bool,
    #[serde(skip, default)]
    screen: Screen,
    #[serde(skip, default)]
    status: String,
}

impl App {
    pub fn new() -> Self {
        App {
            ctx: Default::default(),
            cursor: Position { y: 2, x: 2 },
            cycles: default_cycles(),
            draft: Default::default(),
            help_scroll: 0,
            input_mode: Default::default(),
            nodes: Default::default(),
            op_help: get_help(),
            ops: Default::default(),
            play: Default::default(),
            program: Default::default(),
            recording: Default::default(),
            screen: Default::default(),
            status: Default::default(),
        }
    }

    pub fn node_at_cursor(&self) -> Option<usize> {
        self.nodes.iter().position(
            |Node {
                 position: Position { y, x },
                 op,
                 ..
             }| {
                *y == self.cursor.y
                    && *x <= self.cursor.x
                    // space after node is counted as a part of the node
                    && self.cursor.x <= *x + op.len()
            },
        )
    }

    // TODO Atomic write.
    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let f = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(f, self)?;
        Ok(())
    }

    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let f = std::fs::File::open(path)?;
        Ok(serde_json::from_reader(f)?)
    }
}

#[derive(Serialize, Deserialize)]
enum InputMode {
    Normal,
    Editing,
}

#[derive(Serialize, Deserialize)]
struct Node {
    #[serde(skip, default = "random")]
    id: u64,
    #[serde(skip, default)]
    draft: bool,
    op: String,
    position: Position,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Ord, Deserialize, Serialize)]
struct Position {
    x: usize,
    y: usize,
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let y = self.y.cmp(&other.y);
        Some(if let Ordering::Equal = y {
            self.x.cmp(&other.x)
        } else {
            y
        })
    }
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::Normal
    }
}

enum Screen {
    Editor,
    Help,
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Editor
    }
}

fn default_cycles() -> Vec<Vec<String>> {
    // NOTE Always repeat the first element at the end.
    vec![vec!["s", "t", "w", "c", "s"]]
        .iter()
        .map(|cycle| cycle.iter().map(|s| s.to_string()).collect())
        .collect()
}
