use crate::app::{App, InputMode, Node, Position, Screen, MIN_X, MIN_Y};
use crate::event::{Event, Events};
use anyhow::{anyhow, Result};
use audio_program::{compile_program, rewrite_terms, TextOp};
use audio_vm::VM;
use chrono::prelude::*;
use crossbeam_channel::Sender;
use itertools::Itertools;
use rand::prelude::*;
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
    let mut app = App::load(&filename).unwrap_or_default();
    commit(&mut app, Arc::clone(&vm), sample_rate, filename);
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut events = Events::new();
    loop {
        match app.screen {
            Screen::Editor => render_editor(&mut app, &mut terminal)?,
            Screen::Help => render_help(&mut app, sample_rate, &filename, &mut terminal)?,
            Screen::Ops => render_ops(&mut app, sample_rate, &filename, &mut terminal)?,
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
            Screen::Ops => handle_ops(&mut app, &mut events)?,
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
        for Node {
            id,
            op,
            draft,
            position: p,
        } in app.nodes()
        {
            let text = [Text::raw(op.to_owned())];
            Paragraph::new(text.iter())
                .style(Style::default().fg(if *draft { Color::Red } else { Color::White }))
                .render(
                    &mut f,
                    Rect::new((p.x - 1) as _, (p.y - 1) as _, op.len() as _, 1),
                );
        }

        let color = if !app.play {
            Color::Gray
        } else if app.draft() {
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
        cursor::Goto(app.cursor().x as _, app.cursor().y as _),
    )?;
    match app.input_mode() {
        InputMode::Normal => write!(terminal.backend_mut(), "{}", cursor::SteadyBlock)?,
        InputMode::Insert => write!(terminal.backend_mut(), "{}", cursor::SteadyBar)?,
        InputMode::Replace => write!(terminal.backend_mut(), "{}", cursor::SteadyUnderline)?,
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
            Text::raw(format!("Program: {}\n", app.program())),
            Text::raw(format!("\n")),
            Text::raw(include_str!("help.txt")),
        ];
        size.x = MIN_X as u16;
        size.y = MIN_Y as u16;
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

fn render_ops(
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
            .title("Sound Garden────Ops")
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
            Text::raw(format!("Program: {}\n", app.program())),
            Text::raw(format!("\n")),
            Text::raw(format!("(Press Esc to close, j/k to scroll)\n")),
            Text::raw(format!("\n")),
            Text::raw(
                app.op_groups
                    .iter()
                    .map(|(group, ops)| format!("=== {}\n{}\n", group, ops.join(", ")))
                    .join("\n"),
            ),
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
        Event::Input(input) => match app.input_mode() {
            InputMode::Normal => match input {
                Key::Char('\n') => {
                    commit(app, vm, sample_rate, filename);
                }
                Key::Char('\'') => {
                    app.randomize_node_ids();
                    commit(app, vm, sample_rate, filename);
                }
                Key::Char('\\') => {
                    app.play = !app.play;
                    if app.play {
                        vm.lock().unwrap().play();
                    } else {
                        vm.lock().unwrap().pause();
                    }
                }
                Key::Char('a') => {
                    app.insert_mode();
                    app.move_cursor(Position::x(1));
                    events.disable_exit_key();
                }
                Key::Char('i') => {
                    app.insert_mode();
                    events.disable_exit_key();
                }
                Key::Char('I') => {
                    app.insert_mode();
                    events.disable_exit_key();
                    let new_cursor_x = if let Some(ix) = app.node_at_cursor() {
                        let Node {
                            op, position: p, ..
                        } = &app.nodes()[ix];
                        if p.x == app.cursor().x && op.len() > 1 {
                            p.x
                        } else {
                            p.x + op.len() as i16 + 1
                        }
                    } else {
                        app.cursor().x
                    };
                    app.move_cursor(Position::x(new_cursor_x));
                    if app.node_at_cursor().is_some() {
                        let p = app.cursor();
                        app.move_nodes(
                            app.nodes()
                                .iter()
                                .filter(|node| node.position.y == p.y && node.position.x >= p.x)
                                .map(|node| node.id)
                                .collect(),
                            Position::x(1),
                        );
                    }
                }
                Key::Alt('i') => {
                    app.replace_mode();
                    events.disable_exit_key();
                }
                Key::Char('o') => {
                    app.insert_mode();
                    events.disable_exit_key();
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter_mut()
                            .filter(|node| node.position.y > p.y)
                            .map(|node| node.id)
                            .collect(),
                        Position::y(1),
                    );
                    app.move_cursor(Position {
                        x: app
                            .nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y)
                            .min_by_key(|node| node.position.x)
                            .map(|node| node.position.x)
                            .unwrap_or(MIN_X)
                            - app.cursor().x,
                        y: 1,
                    });
                }
                Key::Char('c') => {
                    app.insert_mode();
                    events.disable_exit_key();
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes()[ix];
                        let push_left = node.position.x + node.op.len() as i16 - app.cursor().x;
                        // TODO command
                        node.op
                            .truncate((app.cursor().x - node.position.x) as usize);
                        node.draft = true;
                        let p = app.cursor();
                        app.move_nodes(
                            app.nodes()
                                .iter()
                                .filter(|node| node.position.y == p.y && node.position.x > p.x)
                                .map(|node| node.id)
                                .collect(),
                            Position::x(-push_left),
                        );
                    }
                }
                Key::Char('h') | Key::Left | Key::Backspace => {
                    app.move_cursor(Position::x(-1));
                }
                Key::Char('j') | Key::Down => {
                    app.move_cursor(Position::y(-1));
                }
                Key::Char('k') | Key::Up => {
                    app.move_cursor(Position::y(1));
                }
                Key::Char('l') | Key::Right | Key::Char(' ') => {
                    app.move_cursor(Position::x(1));
                }
                Key::Alt('h') => {
                    let offset = Position::x(-1);
                    if let Some(ix) = app.node_at_cursor() {
                        app.move_nodes(vec![app.nodes()[ix].id], offset);
                    }
                    app.move_cursor(offset);
                }
                Key::Alt('j') => {
                    let offset = Position::y(1);
                    if let Some(ix) = app.node_at_cursor() {
                        app.move_nodes(vec![app.nodes()[ix].id], offset);
                    }
                    app.move_cursor(offset);
                }
                Key::Alt('k') => {
                    let offset = Position::y(-1);
                    if let Some(ix) = app.node_at_cursor() {
                        app.move_nodes(vec![app.nodes()[ix].id], offset);
                    }
                    app.move_cursor(offset);
                }
                Key::Alt('l') => {
                    let offset = Position::x(1);
                    if let Some(ix) = app.node_at_cursor() {
                        app.move_nodes(vec![app.nodes()[ix].id], offset);
                    }
                    app.move_cursor(offset);
                }
                Key::Char('J') => {
                    let offset = Position::y(1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| {
                                node.position.y > p.y
                                    || node.position.y == p.y
                                        && p.x < node.position.x + node.op.len() as i16
                            })
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('K') => {
                    let offset = Position::y(-1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| {
                                node.position.y < p.y
                                    || node.position.y == p.y && node.position.x <= p.x
                            })
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('H') => {
                    let offset = Position::y(-1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y)
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('L') => {
                    let offset = Position::y(1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y)
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char(',') => {
                    let offset = Position::x(-1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| {
                                node.position.y == p.y
                                    && p.x < node.position.x + node.op.len() as i16
                            })
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('.') => {
                    let offset = Position::x(1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| {
                                node.position.y == p.y
                                    && p.x < node.position.x + node.op.len() as i16
                            })
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('<') => {
                    let offset = Position::x(-1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('>') => {
                    let offset = Position::x(1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('d') => {
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &app.nodes()[ix];
                        app.delete_nodes(vec![node.id]);
                    }
                }
                Key::Char('D') => {
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter_map(|node| {
                            if node.position.y == p.y {
                                Some(node.id)
                            } else {
                                None
                            }
                        })
                        .collect();
                    app.delete_nodes(ids);
                }
                Key::Char('=') => {
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes()[ix];
                        let i = (app.cursor().x - node.position.x) as usize;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 1) % 10;
                            // TODO make a command
                            node.op.replace_range(i..(i + 1), &d.to_string());

                            commit(app, vm, sample_rate, filename);
                        } else {
                            for cycle in &app.cycles {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[0] == node.op) {
                                    // TODO make a command
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
                        let node = &mut app.nodes()[ix];
                        let i = (app.cursor().x - node.position.x) as usize;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 9) % 10;
                            // TODO make a command
                            node.op.replace_range(i..(i + 1), &d.to_string());
                            commit(app, vm, sample_rate, filename);
                        } else {
                            for cycle in &app.cycles {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[1] == node.op) {
                                    // TODO make a command
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
                Key::Char('u') => app.undo(),
                Key::Char('U') => app.redo(),
                Key::Char('?') => app.screen = Screen::Help,
                Key::Char('/') => app.screen = Screen::Ops,
                _ => {}
            },
            InputMode::Insert => match input {
                Key::Left => app.move_cursor(Position::x(-1)),
                Key::Down => app.move_cursor(Position::y(1)),
                Key::Up => app.move_cursor(Position::y(-1)),
                Key::Right => app.move_cursor(Position::x(1)),
                Key::Char(' ') => {
                    let offset = Position::x(1);
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| {
                                node.position.y == p.y
                                    && p.x < node.position.x + node.op.len() as i16
                            })
                            .map(|node| node.id)
                            .collect(),
                        offset,
                    );
                    app.move_cursor(offset);
                }
                Key::Char('\n') => {
                    app.normal_mode();
                    events.enable_exit_key();
                }
                Key::Char(c) => {
                    let p = app.cursor();
                    app.move_nodes(
                        app.nodes()
                            .iter()
                            .filter(|node| node.position.y == p.y && p.x < node.position.x)
                            .map(|node| node.id)
                            .collect(),
                        Position::x(1),
                    );
                    if let Some(ix) = app.node_at_cursor() {
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
                    app.move_cursor(Position::x(1));
                }
                Key::Backspace => {
                    let node_prev_x = app
                        .node_at_cursor()
                        .map(|ix| app.nodes[ix].position.x)
                        .unwrap_or_default();
                    let p = app.cursor;
                    for node in app
                        .nodes
                        .iter_mut()
                        .filter(|node| node.position.y == p.y && p.x <= node.position.x)
                    {
                        node.position.x -= 1;
                    }
                    app.cursor.x -= 1;
                    let node = app.node_at_cursor();
                    if let Some(ix) = node {
                        let node = &mut app.nodes[ix];
                        if node_prev_x == node.position.x
                            && app.cursor.x < node.position.x + node.op.len()
                        {
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
                            } else {
                                node.op.pop();
                            };
                            node.draft = true;
                        }
                    }
                }
                Key::Esc => {
                    app.normal_mode();
                    events.enable_exit_key();
                }
                _ => {}
            },
            InputMode::Replace => match input {
                Key::Left => app.move_cursor(Position::x(-1)),
                Key::Down => app.move_cursor(Position::y(1)),
                Key::Up => app.move_cursor(Position::y(-1)),
                Key::Right => app.move_cursor(Position::x(1)),
                Key::Char(' ') => {
                    if let Some(ix) = app.node_at_cursor() {
                        let node = &mut app.nodes[ix];
                        let new_len = node
                            .op
                            .char_indices()
                            .nth((app.cursor.x - node.position.x) as usize)
                            .map(|x| x.0)
                            .unwrap_or(node.op.len());
                        node.op.truncate(new_len);
                    }
                    app.cursor.x += 1;
                }
                Key::Char('\n') => {
                    app.normal_mode();
                    events.enable_exit_key();
                    app.draft = app.draft || app.nodes.iter().any(|node| node.op.is_empty());
                    app.nodes.retain(|node| !node.op.is_empty());
                }
                Key::Char(c) => {
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
                            node.op.replace_range(ix..(ix + 1), &c.to_string());
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
                    let node_prev_x = app
                        .node_at_cursor()
                        .map(|ix| app.nodes[ix].position.x)
                        .unwrap_or_default();
                    app.cursor.x -= 1;
                    let node = app.node_at_cursor();
                    if let Some(ix) = node {
                        let node = &mut app.nodes[ix];
                        if node_prev_x == node.position.x
                            && app.cursor.x < node.position.x + node.op.len()
                        {
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
                            } else {
                                node.op.pop();
                            };
                            node.draft = true;
                        }
                    }
                }
                Key::Esc => {
                    app.normal_mode();
                    events.enable_exit_key();
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

fn handle_ops(app: &mut App, events: &mut Events) -> Result<()> {
    match events.next()? {
        Event::Input(input) => match input {
            Key::Char('/') => app.screen = Screen::Editor,
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
    app.sort_nodes();
    app.undraft();
    let next_ops = rewrite_terms(
        &app.nodes()
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
