//! microharness — a minimal agentic TUI for a local Ollama server.
//!
//! A chat interface with a status line showing the current model and think
//! level. Type a prompt and press Enter to send; the reply streams into the
//! transcript. `/models` opens the model picker, `/think <level>` sets the
//! think level (auto/on/off/low/medium/high/max), and `Ctrl-T` cycles it.
//! Ctrl-C quits.

mod ollama;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use ollama::{Ollama, Think};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::sync::mpsc;

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "llama3.2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::User => "you",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug)]
enum Event {
    Chunk(String),
    Reply(Result<String>),
    Models(Vec<String>),
    ModelsError(anyhow::Error),
}

struct App {
    ollama: Ollama,
    base_url: String,
    model: String,
    think: Think,
    input: String,
    transcript: Vec<(Role, String)>,
    /// Partial reply currently streaming (Some while streaming, None when idle).
    streaming: Option<String>,
    error: Option<String>,
    /// Model picker state.
    picker_open: bool,
    models: Vec<String>,
    picker_query: String,
    selected: usize,
}

impl App {
    fn new(ollama: Ollama, base_url: String, model: String) -> Self {
        Self {
            ollama,
            base_url,
            model,
            think: Think::Auto,
            input: String::new(),
            transcript: Vec::new(),
            streaming: None,
            error: None,
            picker_open: false,
            models: Vec::new(),
            picker_query: String::new(),
            selected: 0,
        }
    }

    fn filtered_models(&self) -> Vec<&str> {
        if self.picker_query.is_empty() {
            return self.models.iter().map(|m| m.as_str()).collect();
        }
        let q = self.picker_query.to_lowercase();
        self.models
            .iter()
            .filter(|m| m.to_lowercase().contains(&q))
            .map(|m| m.as_str())
            .collect()
    }

    fn status_spans(&self) -> Vec<Span<'static>> {
        let model = self.model.clone();
        let base_url = self.base_url.clone();
        let think = self.think.label().to_string();
        vec![
            Span::styled(
                " microharness ",
                Style::new().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::raw(" "),
            Span::styled(
                model,
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" @ "),
            Span::styled(base_url, Style::new().fg(Color::DarkGray)),
            Span::raw("  ·  "),
            Span::styled("think", Style::new().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(
                think,
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  /models  [Ctrl-T] think  [Enter] send  [Ctrl-C] quit"),
        ]
    }

    fn render_transcript(&self) -> Text<'_> {
        let mut text = Text::default();
        for (role, content) in &self.transcript {
            text.push_line(Line::styled(
                role.label().to_string(),
                Style::new().fg(if *role == Role::User {
                    Color::Green
                } else {
                    Color::Blue
                }),
            ));
            for line in content.lines() {
                text.push_line(Line::raw(line.to_string()));
            }
            text.push_line(Line::raw(""));
        }
        if let Some(partial) = &self.streaming {
            text.push_line(Line::styled("assistant", Style::new().fg(Color::Blue)));
            for line in partial.lines() {
                text.push_line(Line::raw(line.to_string()));
            }
            text.push_line(Line::raw(""));
        }
        if let Some(err) = &self.error {
            text.push_line(Line::styled(err.clone(), Style::new().fg(Color::Red)));
        }
        text
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut base_url = DEFAULT_BASE_URL.to_string();
    let mut model = DEFAULT_MODEL.to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--base-url" => {
                i += 1;
                base_url = args.get(i).cloned().unwrap_or_default();
            }
            "--model" => {
                i += 1;
                model = args.get(i).cloned().unwrap_or_default();
            }
            other => return Err(anyhow::anyhow!("unknown argument: {other}")),
        }
        i += 1;
    }

    let ollama = Ollama::new(&base_url, &model);
    let app = App::new(ollama, base_url, model);
    let mut terminal = ratatui::init();
    let res = run(&mut terminal, app).await;
    ratatui::restore();
    res
}

async fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let mut events = crossterm::event::EventStream::new();

    loop {
        terminal.draw(|f| ui(f, &app))?;
        tokio::select! {
            maybe = events.next() => match maybe {
                Some(Ok(crossterm::event::Event::Key(key))) => {
                    if handle_key(&tx, &mut app, key).await? {
                        break;
                    }
                }
                Some(Err(e)) => return Err(e.into()),
                _ => {}
            },
            Some(event) = rx.recv() => match event {
                Event::Chunk(chunk) => {
                    if let Some(s) = app.streaming.as_mut() {
                        s.push_str(&chunk);
                    }
                }
                Event::Reply(Ok(full)) => {
                    app.streaming = None;
                    app.transcript.push((Role::Assistant, full));
                }
                Event::Reply(Err(e)) => {
                    app.streaming = None;
                    app.error = Some(format!("error: {e:#}"));
                }
                Event::Models(models) => {
                    if !models.is_empty() {
                        app.models = models;
                        if app.selected >= app.models.len() {
                            app.selected = 0;
                        }
                    }
                }
                Event::ModelsError(e) => {
                    app.picker_open = false;
                    app.error = Some(format!("failed to list models: {e:#}"));
                }
            },
        }
    }
    Ok(())
}

async fn handle_key(
    tx: &mpsc::UnboundedSender<Event>,
    app: &mut App,
    key: KeyEvent,
) -> Result<bool> {
    // Modal model picker.
    if app.picker_open {
        let filtered = app.filtered_models();
        match key.code {
            KeyCode::Down => {
                app.selected = (app.selected + 1).min(filtered.len().saturating_sub(1));
            }
            KeyCode::Up => {
                app.selected = app.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(name) = filtered.get(app.selected) {
                    app.model = (*name).to_string();
                }
                app.picker_open = false;
            }
            KeyCode::Esc => app.picker_open = false,
            KeyCode::Backspace => {
                app.picker_query.pop();
                app.selected = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.picker_query.push(c);
                app.selected = 0;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.think = app.think.next();
            return Ok(false);
        }
        KeyCode::Enter => {
            let prompt = app.input.trim().to_string();
            if app.streaming.is_none() && !prompt.is_empty() {
                app.input.clear();
                if let Some(cmd) = prompt.strip_prefix('/') {
                    handle_command(tx, app, cmd);
                } else {
                    app.transcript.push((Role::User, prompt.clone()));
                    app.streaming = Some(String::new());
                    app.error = None;
                    start_chat(tx.clone(), app.ollama.clone(), &app.transcript, app.think);
                }
            }
        }
        KeyCode::Char(c) => app.input.push(c),
        KeyCode::Backspace => {
            app.input.pop();
        }
        _ => {}
    }
    Ok(false)
}

/// Handle a `/command` typed in the input. Called with the command name and
/// any trailing arguments (already stripped of the leading `/`).
fn handle_command(tx: &mpsc::UnboundedSender<Event>, app: &mut App, raw: &str) {
    let cmd = raw.split_whitespace().next().unwrap_or("");
    match cmd {
        "models" => {
            app.picker_open = true;
            app.models.clear();
            app.picker_query.clear();
            app.selected = 0;
            app.error = None;
            open_picker(tx.clone(), app.ollama.clone());
        }
        "think" => {
            let level = raw.split_whitespace().nth(1);
            match level.and_then(parse_think) {
                Some(think) => app.think = think,
                None => {
                    app.error = Some(
                        "usage: /think auto|on|off|low|medium|high|max (or press Ctrl-T to cycle)"
                            .to_string(),
                    )
                }
            }
        }
        "help" => {
            app.error = Some(
                "commands: /models · /think auto|on|off|low|medium|high|max · /help".to_string(),
            );
        }
        other => {
            app.error = Some(format!("unknown command: /{other} (try /help)"));
        }
    }
}

fn parse_think(s: &str) -> Option<Think> {
    match s.to_ascii_lowercase().as_str() {
        "auto" => Some(Think::Auto),
        "on" => Some(Think::On),
        "off" => Some(Think::Off),
        "low" => Some(Think::Low),
        "medium" => Some(Think::Medium),
        "high" => Some(Think::High),
        "max" => Some(Think::Max),
        _ => None,
    }
}

fn open_picker(tx: mpsc::UnboundedSender<Event>, ollama: Ollama) {
    tokio::spawn(async move {
        match ollama.list_models().await {
            Ok(models) => {
                let _ = tx.send(Event::Models(models));
            }
            Err(e) => {
                let _ = tx.send(Event::ModelsError(e));
            }
        }
    });
}

fn start_chat(
    tx: mpsc::UnboundedSender<Event>,
    ollama: Ollama,
    transcript: &[(Role, String)],
    think: Think,
) {
    let mut ollama = ollama;
    ollama.set_think(think);
    let history: Vec<(String, String)> = transcript
        .iter()
        .map(|(role, content)| (role.label().to_string(), content.clone()))
        .collect();

    tokio::spawn(async move {
        let result = ollama
            .chat(&history, |delta| {
                let _ = tx.send(Event::Chunk(delta.to_string()));
            })
            .await;
        let _ = tx.send(Event::Reply(result));
    });
}

fn ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let transcript = Paragraph::new(app.render_transcript())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" conversation "),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(transcript, chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(" input "));
    frame.render_widget(input, chunks[1]);
    frame.set_cursor_position(ratatui::layout::Position::new(
        chunks[1].x + 1 + app.input.chars().count() as u16,
        chunks[1].y + 1,
    ));

    let status = Paragraph::new(Line::from(app.status_spans()));
    frame.render_widget(status, chunks[2]);

    if app.picker_open {
        let area = centered_rect(60, 60, frame.area());
        frame.render_widget(Clear, area);

        if app.models.is_empty() {
            let loading = Paragraph::new("Loading models…")
                .block(Block::default().borders(Borders::ALL).title(" models "));
            frame.render_widget(loading, area);
        } else {
            let filtered = app.filtered_models();
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(area);

            let query = Paragraph::new(app.picker_query.as_str())
                .block(Block::default().borders(Borders::ALL).title(" search "));
            frame.render_widget(query, inner[0]);
            frame.set_cursor_position(ratatui::layout::Position::new(
                inner[0].x + 1 + app.picker_query.chars().count() as u16,
                inner[0].y + 1,
            ));

            let items: Vec<ListItem> = filtered
                .iter()
                .enumerate()
                .map(|(idx, name)| {
                    let style = if idx == app.selected {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new((*name).to_string()).style(style)
                })
                .collect();
            let mut state = ListState::default();
            state.select(Some(app.selected));
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" models  [type] filter  [↑/↓] select  [Enter] use  [Esc] close "),
                )
                .highlight_style(Style::default().bg(Color::DarkGray))
                .highlight_symbol("> ");
            frame.render_stateful_widget(list, inner[1], &mut state);
        }
    }
}

/// A rectangle centered within `r`, covering `percent_x` × `percent_y` of it.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
