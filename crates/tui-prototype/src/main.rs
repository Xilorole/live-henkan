//! TUI prototype for live-henkan — interactive Japanese IME in the terminal.
//!
//! Uses ratatui + crossterm to render a simple UI showing:
//! - Committed text (green)
//! - Live conversion result (yellow, underlined)
//! - Pending romaji (dim)
//!
//! Press Escape or Ctrl-C to quit, Enter to commit, Backspace to delete.

use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use dictionary::{ConnectionCost, Dictionary};
use engine::LiveEngine;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Default path to IPAdic dictionary data.
const DEFAULT_DICT_DIR: &str = "data/dictionary/mecab-ipadic-2.7.0-20070801";

fn main() -> io::Result<()> {
    let dict_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DICT_DIR));

    eprintln!("Loading dictionary from {}...", dict_dir.display());

    let dict = Dictionary::load_from_dir(&dict_dir).unwrap_or_else(|e| {
        eprintln!("Failed to load dictionary: {e}");
        eprintln!("Run ./scripts/setup-dict.sh first to download dictionary data.");
        std::process::exit(1);
    });

    let matrix_path = dict_dir.join("matrix.def");
    let matrix_file = std::fs::File::open(&matrix_path).unwrap_or_else(|e| {
        eprintln!("Failed to open {}: {e}", matrix_path.display());
        std::process::exit(1);
    });
    let conn = ConnectionCost::from_reader(io::BufReader::new(matrix_file)).unwrap_or_else(|e| {
        eprintln!("Failed to parse matrix.def: {e}");
        std::process::exit(1);
    });

    eprintln!(
        "Dictionary loaded ({} readings). Starting TUI...",
        dict.len()
    );

    let mut engine = LiveEngine::new(dict, conn);

    // Setup terminal
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let result = run_app(&mut terminal, &mut engine);

    // Restore terminal
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

/// Application state for the TUI.
struct AppState {
    /// All committed text accumulated.
    committed_text: String,
    /// Current composing text (live conversion result).
    composing: String,
    /// Pending romaji not yet converted to hiragana.
    pending: String,
    /// Status message.
    status: String,
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: &mut LiveEngine,
) -> io::Result<()> {
    let mut state = AppState {
        committed_text: String::new(),
        composing: String::new(),
        pending: String::new(),
        status: "Type romaji to convert. Enter=commit, Esc=quit".into(),
    };

    loop {
        terminal.draw(|frame| draw(frame, &state))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Enter => {
                    let committed = engine.commit();
                    state.committed_text.push_str(&committed);
                    state.composing.clear();
                    state.pending.clear();
                    state.status = format!("Committed: {committed}");
                }
                KeyCode::Backspace => {
                    if !state.composing.is_empty() || !state.pending.is_empty() {
                        let output = engine.backspace();
                        state.composing = output.composing;
                        state.pending = output.raw_pending;
                        state.status = format!(
                            "hiragana: {} | pending: {}",
                            engine.hiragana_buffer(),
                            state.pending
                        );
                    } else if !state.committed_text.is_empty() {
                        // Delete last char from committed text
                        state.committed_text.pop();
                        state.status = "Deleted last character".into();
                    }
                }
                KeyCode::Char(ch) if ch.is_ascii_alphabetic() => {
                    let output = engine.on_key(ch);
                    if !output.committed.is_empty() {
                        state.committed_text.push_str(&output.committed);
                    }
                    state.composing = output.composing;
                    state.pending = output.raw_pending;
                    state.status = format!(
                        "hiragana: {} | pending: {}",
                        engine.hiragana_buffer(),
                        state.pending
                    );
                }
                KeyCode::Char(ch) => {
                    // Non-alphabetic: commit current + map punctuation
                    let committed = engine.commit();
                    state.committed_text.push_str(&committed);
                    let mapped = match ch {
                        '.' => '。',
                        ',' => '、',
                        '!' => '！',
                        '?' => '？',
                        '(' => '（',
                        ')' => '）',
                        _ => ch,
                    };
                    state.committed_text.push(mapped);
                    state.composing.clear();
                    state.pending.clear();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(5),    // Main input area
            Constraint::Length(3), // Status bar
        ])
        .split(area);

    // Title
    let title = Paragraph::new("live-henkan TUI prototype")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, layout[0]);

    // Main display: committed + composing + pending
    let mut spans = Vec::new();

    if !state.committed_text.is_empty() {
        spans.push(Span::styled(
            &state.committed_text,
            Style::default().fg(Color::Green),
        ));
    }

    if !state.composing.is_empty() {
        spans.push(Span::styled(
            &state.composing,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::UNDERLINED),
        ));
    }

    if !state.pending.is_empty() {
        spans.push(Span::styled(
            &state.pending,
            Style::default().fg(Color::DarkGray),
        ));
    }

    if spans.is_empty() {
        spans.push(Span::styled(
            "▌",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ));
    }

    let input_line = Line::from(spans);
    let input_area =
        Paragraph::new(input_line).block(Block::default().borders(Borders::ALL).title("Input"));
    frame.render_widget(input_area, layout[1]);

    // Status bar
    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(status, layout[2]);
}
