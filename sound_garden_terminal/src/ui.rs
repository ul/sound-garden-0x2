use crate::app::{App, InputMode, Node, Position, Screen, MIN_X, MIN_Y};
use crate::event::{Event, Events};
use anyhow::{anyhow, Result};
use audio_program::TextOp;
use chrono::prelude::*;
use crossbeam_channel::Sender;
use itertools::Itertools;
use std::io::{self, Write};
use termion::cursor;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::Rect;
use tui::style::{Color, Style};
use tui::widgets::{Block, Borders, Paragraph, Text};
use tui::Terminal;

pub fn main(
    tx_ops: Sender<Vec<TextOp>>,
    tx_play: Sender<bool>,
    sample_rate: u32,
    filename: &str,
    record_tx: &Sender<bool>,
) -> Result<()> {
    let mut app = App::load(filename, tx_ops)?;
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
            Screen::Editor => handle_editor(&mut app, &mut events, record_tx, &tx_play)?,
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
            op,
            draft,
            position: p,
            ..
        } in app.nodes()
        {
            let text = [Text::raw(op.to_owned())];
            f.render_widget(
                Paragraph::new(text.iter()).style(Style::default().fg(if *draft {
                    Color::Red
                } else {
                    Color::White
                })),
                Rect::new((p.x - 1) as _, (p.y - 1) as _, op.len() as _, 1),
            )
        }

        let color = if !app.play() {
            Color::Gray
        } else if app.draft() {
            Color::Red
        } else {
            Color::White
        };
        f.render_widget(
            Block::default()
                .title(&format!(
                    "Sound Garden────{}────{}────{}",
                    if app.play() { "|>" } else { "||" },
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
                .border_style(Style::default().fg(color)),
            size,
        );
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
        f.render_widget(
            Block::default()
                .title("Sound Garden────Help")
                .title_style(Style::default().fg(Color::Green))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
            size,
        );
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
        f.render_widget(
            Paragraph::new(text.iter())
                .scroll(app.help_scroll)
                .wrap(true),
            size,
        );
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
        f.render_widget(
            Block::default()
                .title("Sound Garden────Ops")
                .title_style(Style::default().fg(Color::Green))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
            size,
        );
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
        f.render_widget(
            Paragraph::new(text.iter())
                .scroll(app.help_scroll)
                .wrap(true),
            size,
        );
    })?;
    write!(terminal.backend_mut(), "{}", cursor::Hide,)?;
    io::stdout().flush()?;
    Ok(())
}

fn handle_editor(
    app: &mut App,
    events: &mut Events,
    record_tx: &Sender<bool>,
    tx_play: &Sender<bool>,
) -> Result<()> {
    match events.next()? {
        Event::Input(input) => match app.input_mode() {
            InputMode::Normal => match input {
                Key::Char('\n') => {
                    app.commit();
                }
                Key::Char('\'') => {
                    app.randomize_node_ids();
                    app.commit();
                }
                Key::Char('\\') => {
                    app.toggle_play();
                    tx_play.send(app.play()).ok();
                }
                Key::Char('a') => {
                    events.disable_exit_key();
                    app.move_cursor(Position::x(1));
                    app.insert_mode();
                }
                Key::Char('i') => {
                    events.disable_exit_key();
                    app.insert_mode();
                }
                Key::Char('I') => {
                    events.disable_exit_key();
                    app.splash();
                    app.insert_mode();
                }
                Key::Char('o') => {
                    events.disable_exit_key();
                    app.insert_line();
                    app.insert_mode();
                }
                Key::Char('c') => {
                    events.disable_exit_key();
                    app.cut_op();
                    app.insert_mode();
                }
                Key::Char('h') | Key::Left | Key::Backspace => {
                    app.move_cursor(Position::x(-1));
                }
                Key::Char('j') | Key::Down => {
                    app.move_cursor(Position::y(1));
                }
                Key::Char('k') | Key::Up => {
                    app.move_cursor(Position::y(-1));
                }
                Key::Char('l') | Key::Right | Key::Char(' ') => {
                    app.move_cursor(Position::x(1));
                }
                Key::Alt('h') => {
                    let offset = Position::x(-1);
                    let mut ids = Vec::new();
                    if let Some(id) = app.node_at_cursor().map(|node| node.id) {
                        ids.push(id);
                    }
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Alt('j') => {
                    let offset = Position::y(1);
                    let mut ids = Vec::new();
                    if let Some(id) = app.node_at_cursor().map(|node| node.id) {
                        ids.push(id);
                    }
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Alt('k') => {
                    let offset = Position::y(-1);
                    let mut ids = Vec::new();
                    if let Some(id) = app.node_at_cursor().map(|node| node.id) {
                        ids.push(id);
                    }
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Alt('l') => {
                    let offset = Position::x(1);
                    let mut ids = Vec::new();
                    if let Some(id) = app.node_at_cursor().map(|node| node.id) {
                        ids.push(id);
                    }
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('J') => {
                    let offset = Position::y(1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| {
                            node.position.y > p.y
                                || node.position.y == p.y
                                    && p.x < node.position.x + node.op.len() as i16
                        })
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('K') => {
                    let offset = Position::y(-1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| {
                            node.position.y < p.y
                                || node.position.y == p.y && node.position.x <= p.x
                        })
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('H') => {
                    let offset = Position::y(-1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| node.position.y == p.y)
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('L') => {
                    let offset = Position::y(1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| node.position.y == p.y)
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char(',') => {
                    let offset = Position::x(-1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| {
                            node.position.y == p.y && p.x < node.position.x + node.op.len() as i16
                        })
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('.') => {
                    let offset = Position::x(1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| {
                            node.position.y == p.y && p.x < node.position.x + node.op.len() as i16
                        })
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('<') => {
                    let offset = Position::x(-1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('>') => {
                    let offset = Position::x(1);
                    let p = app.cursor();
                    let ids = app
                        .nodes()
                        .iter()
                        .filter(|node| node.position.y == p.y && node.position.x <= p.x)
                        .map(|node| node.id)
                        .collect();
                    app.move_nodes_and_cursor(ids, offset, offset);
                }
                Key::Char('d') => {
                    if let Some(id) = app.node_at_cursor().map(|node| node.id) {
                        app.delete_nodes(vec![id]);
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
                    if let Some(node) = app.node_at_cursor() {
                        let i = (app.cursor().x - node.position.x) as usize;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 1) % 10;
                            let mut op = node.op.to_owned();
                            op.replace_range(i..(i + 1), &d.to_string());
                            let id = node.id;
                            app.replace_op(id, op);
                            app.commit();
                        } else {
                            for cycle in &app.cycles.to_owned() {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[0] == node.op) {
                                    let id = node.id;
                                    app.replace_op(id, ops[1].to_owned());
                                    app.commit();
                                    break;
                                }
                            }
                        }
                    }
                }
                Key::Char('-') => {
                    if let Some(node) = app.node_at_cursor() {
                        let i = (app.cursor().x - node.position.x) as usize;
                        if let Some(d) = node.op.get(i..(i + 1)).and_then(|c| c.parse::<u8>().ok())
                        {
                            let d = (d + 9) % 10;
                            let mut op = node.op.to_owned();
                            op.replace_range(i..(i + 1), &d.to_string());
                            let id = node.id;
                            app.replace_op(id, op);
                            app.commit();
                        } else {
                            for cycle in &app.cycles.to_owned() {
                                if let Some(ops) = cycle.windows(2).find(|ops| ops[1] == node.op) {
                                    let id = node.id;
                                    app.replace_op(id, ops[0].to_owned());
                                    app.commit();
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
                    app.insert_space();
                }
                Key::Char('\n') => {
                    events.enable_exit_key();
                    app.normal_mode();
                }
                Key::Char(c) => {
                    app.insert_char(c);
                }
                Key::Backspace => {
                    app.delete_char();
                }
                Key::Esc => {
                    events.enable_exit_key();
                    app.normal_mode();
                }
                _ => {}
            },
        },
        _ => {}
    }
    // TODO Update on undo/redo Signal instead?
    app.update();
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
