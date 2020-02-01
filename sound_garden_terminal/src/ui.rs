use crate::event::{Event, Events};
use anyhow::Result;
use audio_program::{parse_tokens, Context};
use audio_vm::VM;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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

pub fn run<P: AsRef<std::path::Path>>(
    vm: Arc<Mutex<VM>>,
    sample_rate: u32,
    filename: P,
) -> Result<()> {
    let mut app = App::load(&filename).unwrap_or_else(|_| App::new());
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut events = Events::new();
    write!(terminal.backend_mut(), "{}", cursor::Show)?;
    let commit = |app: &mut App| {
        app.nodes.sort_by_key(|node| node.position);
        app.nodes.iter_mut().for_each(|node| node.draft = false);
        let next_ops = app
            .nodes
            .iter()
            .map(|Node { id, op, .. }| CachedOp {
                id: *id,
                op: op.to_owned(),
            })
            .collect::<Vec<_>>();

        if app.ops != next_ops {
            let prg = next_ops.iter().map(|x| x.op.to_owned()).collect::<Vec<_>>();
            let new_program = parse_tokens(&prg, sample_rate, &mut app.ctx);
            let reuse = next_ops
                .iter()
                .enumerate()
                .filter_map(|(n, op)| app.ops.iter().position(|x| x == op).map(|p| (p, n)))
                .collect::<Vec<_>>();
            // Ensure the smallest possible scope to limit locking time.
            {
                vm.lock().unwrap().load_program_reuse(new_program, &reuse);
            }
            app.ops = next_ops;
            app.save(&filename).ok();
        }
    };
    loop {
        terminal.draw(|mut f| {
            let size = f.size();
            Block::default()
                .title("Sound Garden")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(match app.input_mode {
                    InputMode::Normal => Color::White,
                    InputMode::Editing => Color::Green,
                }))
                .render(&mut f, size);
            for Node {
                op,
                draft,
                position: p,
                ..
            } in app.nodes.iter()
            {
                let text = [Text::raw(op.to_owned())];
                Paragraph::new(text.iter())
                    .style(Style::default().fg(if *draft { Color::Red } else { Color::White }))
                    .render(
                        &mut f,
                        Rect::new((p.x - 1) as _, (p.y - 1) as _, op.len() as _, 1),
                    );
            }
        })?;

        write!(
            terminal.backend_mut(),
            "{}",
            cursor::Goto(app.cursor.x as _, app.cursor.y as _),
        )?;
        match app.input_mode {
            InputMode::Normal => write!(terminal.backend_mut(), "{}", cursor::SteadyBlock)?,
            InputMode::Editing => write!(terminal.backend_mut(), "{}", cursor::SteadyUnderline)?,
        }
        io::stdout().flush()?;

        match events.next()? {
            Event::Input(input) => match app.input_mode {
                InputMode::Normal => match input {
                    Key::Char('\n') => commit(&mut app),
                    Key::Char('\\') => {
                        let mut vm = vm.lock().unwrap();
                        vm.pause = !vm.pause;
                    }
                    Key::Char('i') => {
                        app.input_mode = InputMode::Editing;
                        events.disable_exit_key();
                        if app.node_at_cursor().is_none() {
                            let node = Node {
                                id: random(),
                                draft: true,
                                op: String::new(),
                                position: app.cursor,
                            };
                            app.nodes.push(node);
                        }
                    }
                    Key::Char('c') => {
                        app.input_mode = InputMode::Editing;
                        events.disable_exit_key();
                        match app.node_at_cursor() {
                            Some(ix) => {
                                let node = &mut app.nodes[ix];
                                node.op.truncate(app.cursor.x - node.position.x);
                                node.draft = true;
                            }
                            None => {
                                let node = Node {
                                    id: random(),
                                    draft: true,
                                    op: String::new(),
                                    position: app.cursor,
                                };
                                app.nodes.push(node);
                            }
                        }
                    }
                    Key::Char('h') | Key::Left => {
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
                    Key::Char('H') => {
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
                            node.position.y < p.y
                                || node.position.y == p.y && node.position.x <= p.x
                        }) {
                            node.position.y -= 1;
                        }
                        app.cursor.y -= 1;
                    }
                    Key::Char('L') => {
                        let p = app.cursor;
                        for node in app.nodes.iter_mut().filter(|node| {
                            node.position.y == p.y && p.x < node.position.x + node.op.len()
                        }) {
                            node.position.x += 1;
                        }
                        app.cursor.x += 1;
                    }
                    Key::Char('d') => {
                        if let Some(ix) = app.node_at_cursor() {
                            app.nodes.swap_remove(ix);
                        }
                    }
                    Key::Char('=') => {
                        if let Some(ix) = app.node_at_cursor() {
                            if let Ok(x) = app.nodes[ix].op.parse::<f64>() {
                                app.nodes[ix].op = format!("{}", x + 1.0);
                                commit(&mut app);
                            };
                        }
                    }
                    Key::Char('-') => {
                        if let Some(ix) = app.node_at_cursor() {
                            if let Ok(x) = app.nodes[ix].op.parse::<f64>() {
                                app.nodes[ix].op = format!("{}", x - 1.0);
                                commit(&mut app);
                            };
                        }
                    }
                    Key::Char('+') => {
                        if let Some(ix) = app.node_at_cursor() {
                            if let Ok(x) = app.nodes[ix].op.parse::<f64>() {
                                app.nodes[ix].op = format!("{}", x + 10.0);
                                commit(&mut app);
                            };
                        }
                    }
                    Key::Char('_') => {
                        if let Some(ix) = app.node_at_cursor() {
                            if let Ok(x) = app.nodes[ix].op.parse::<f64>() {
                                app.nodes[ix].op = format!("{}", x - 10.0);
                                commit(&mut app);
                            };
                        }
                    }
                    Key::Char('q') => {
                        break;
                    }
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
                        if app.node_at_cursor().is_none() {
                            let node = Node {
                                id: random(),
                                draft: true,
                                op: String::new(),
                                position: app.cursor,
                            };
                            app.nodes.push(node);
                        }
                    }
                    Key::Char('\n') => {
                        app.input_mode = InputMode::Normal;
                        events.enable_exit_key();
                        if let Some(ix) = app.node_at_cursor() {
                            if app.nodes[ix].op.is_empty() {
                                app.nodes.swap_remove(ix);
                            }
                        }
                    }
                    Key::Char(c) => {
                        let node = app.node_at_cursor();
                        if let Some(ix) = node {
                            let p = app.cursor;
                            for node in app
                                .nodes
                                .iter_mut()
                                .filter(|node| node.position.y == p.y && p.x < node.position.x)
                            {
                                node.position.x += 1;
                            }
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
                        let node = app.node_at_cursor();
                        app.cursor.x -= 1;
                        if let Some(ix) = node {
                            let p = app.cursor;
                            for node in app.nodes.iter_mut().filter(|node| {
                                node.position.y == p.y && node.position.x + node.op.len() < p.x
                            }) {
                                node.position.x -= 1;
                            }
                            let node = &mut app.nodes[ix];
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
                                node.draft = true;
                            } else {
                                app.nodes.swap_remove(ix);
                            };
                        }
                    }
                    Key::Esc => {
                        app.input_mode = InputMode::Normal;
                        events.enable_exit_key();
                    }
                    _ => {}
                },
            },
            _ => {}
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct App {
    #[serde(skip, default)]
    ctx: Context,
    #[serde(skip, default)]
    ops: Vec<CachedOp>,
    nodes: Vec<Node>,
    #[serde(skip, default)]
    input_mode: InputMode,
    cursor: Position,
}

impl App {
    pub fn new() -> Self {
        App {
            ctx: Default::default(),
            ops: Default::default(),
            nodes: Default::default(),
            input_mode: Default::default(),
            cursor: Position { y: 2, x: 2 },
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

#[derive(Clone, PartialEq, Eq, Default)]
struct CachedOp {
    id: u64,
    op: String,
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
