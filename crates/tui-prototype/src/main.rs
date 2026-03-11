//! TUI prototype for live-henkan — interactive Japanese IME in the terminal.
//!
//! Uses ratatui + crossterm to render a simple UI showing:
//! - Committed text (green)
//! - Live conversion result (yellow, underlined) with segment highlighting
//! - Pending romaji (dim)
//! - Candidate selection popup when in selection mode
//!
//! Press Escape or Ctrl-C to quit, Enter to commit, Backspace to delete.
//! Space enters candidate selection mode; arrow keys navigate.

use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use dictionary::{ConnectionCost, Dictionary};
use engine::{EngineMode, LiveEngine};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

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
    /// Candidate surface strings for display (only in selection mode).
    candidates: Vec<String>,
    /// Index of the highlighted candidate.
    selected_candidate: usize,
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: &mut LiveEngine,
) -> io::Result<()> {
    let mut state = AppState {
        committed_text: String::new(),
        composing: String::new(),
        pending: String::new(),
        status: "Type romaji to convert. Enter=commit, Space=candidates, Esc=quit".into(),
        candidates: Vec::new(),
        selected_candidate: 0,
    };

    loop {
        terminal.draw(|frame| draw(frame, &state, engine))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => {
                    if *engine.mode() == EngineMode::Selecting {
                        engine.cancel_selection();
                        sync_state(&mut state, engine);
                    } else {
                        break;
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Enter => {
                    if *engine.mode() == EngineMode::Selecting {
                        let committed = engine.confirm_selection();
                        state.committed_text.push_str(&committed);
                        state.composing.clear();
                        state.pending.clear();
                        state.candidates.clear();
                        state.status = format!("Committed: {committed}");
                    } else {
                        let committed = engine.commit();
                        state.committed_text.push_str(&committed);
                        state.composing.clear();
                        state.pending.clear();
                        state.status = format!("Committed: {committed}");
                    }
                }
                KeyCode::Char(' ') => {
                    if *engine.mode() == EngineMode::Selecting {
                        // Already selecting → next candidate
                        engine.next_candidate();
                        sync_state(&mut state, engine);
                    } else if !state.composing.is_empty() {
                        // Enter selection mode
                        if engine.enter_selection() {
                            sync_state(&mut state, engine);
                            state.status =
                                "Space/\u{2193}=next  \u{2191}=prev  \u{2190}\u{2192}=segment  Shift+\u{2190}\u{2192}=resize  Enter=confirm  Esc=cancel".into();
                        }
                    } else {
                        // No composing text → insert space
                        state.committed_text.push(' ');
                    }
                }
                KeyCode::Down => {
                    if *engine.mode() == EngineMode::Selecting {
                        engine.next_candidate();
                        sync_state(&mut state, engine);
                    }
                }
                KeyCode::Up => {
                    if *engine.mode() == EngineMode::Selecting {
                        engine.prev_candidate();
                        sync_state(&mut state, engine);
                    }
                }
                KeyCode::Right => {
                    if *engine.mode() == EngineMode::Selecting {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            engine.extend_segment();
                        } else {
                            engine.next_segment();
                        }
                        sync_state(&mut state, engine);
                    }
                }
                KeyCode::Left => {
                    if *engine.mode() == EngineMode::Selecting {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            engine.shrink_segment();
                        } else {
                            engine.prev_segment();
                        }
                        sync_state(&mut state, engine);
                    }
                }
                KeyCode::Backspace => {
                    if *engine.mode() == EngineMode::Selecting {
                        engine.cancel_selection();
                        sync_state(&mut state, engine);
                    } else if !state.composing.is_empty() || !state.pending.is_empty() {
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
                    state.candidates.clear();
                    state.status = format!(
                        "hiragana: {} | pending: {}",
                        engine.hiragana_buffer(),
                        state.pending
                    );
                }
                KeyCode::Char(ch) => {
                    // Non-alphabetic: commit current + map punctuation
                    if *engine.mode() == EngineMode::Selecting {
                        let committed = engine.confirm_selection();
                        state.committed_text.push_str(&committed);
                    } else {
                        let committed = engine.commit();
                        state.committed_text.push_str(&committed);
                    }
                    let mapped = match ch {
                        '.' => '\u{3002}',
                        ',' => '\u{3001}',
                        '!' => '\u{FF01}',
                        '?' => '\u{FF1F}',
                        '(' => '\u{FF08}',
                        ')' => '\u{FF09}',
                        _ => ch,
                    };
                    state.committed_text.push(mapped);
                    state.composing.clear();
                    state.pending.clear();
                    state.candidates.clear();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Sync TUI state from engine after a selection-mode operation.
fn sync_state(state: &mut AppState, engine: &LiveEngine) {
    let segs = engine.display_segments();
    state.composing = segs.iter().map(|s| s.surface.as_str()).collect();
    state.pending = String::new();

    // Update candidate list
    let candidates = engine.current_candidates();
    state.candidates = candidates.iter().map(|c| c.surface.clone()).collect();
    state.selected_candidate = engine.active_candidate_index();
}

fn draw(frame: &mut Frame, state: &AppState, engine: &LiveEngine) {
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

    // Main display: committed + composing (with segment highlighting) + pending
    let mut spans = Vec::new();

    if !state.committed_text.is_empty() {
        spans.push(Span::styled(
            &state.committed_text,
            Style::default().fg(Color::Green),
        ));
    }

    let in_selection = *engine.mode() == EngineMode::Selecting;
    let display_segs = engine.display_segments();

    if in_selection && !display_segs.is_empty() {
        // Show each segment separately, highlight the active one
        for seg in &display_segs {
            let style = if seg.is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::UNDERLINED)
            };
            spans.push(Span::styled(&seg.surface, style));
        }
    } else if !state.composing.is_empty() {
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

    // Candidate popup (drawn on top of input area when in selection mode)
    if in_selection && !state.candidates.is_empty() {
        let max_display = 10.min(state.candidates.len());
        let items: Vec<ListItem> = state
            .candidates
            .iter()
            .enumerate()
            .take(max_display)
            .map(|(i, surface)| {
                let marker = if i == state.selected_candidate {
                    "▸ "
                } else {
                    "  "
                };
                let style = if i == state.selected_candidate {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(format!("{marker}{i}. {surface}")).style(style)
            })
            .collect();

        // Position popup below the input area
        let popup_height = (max_display as u16 + 2).min(area.height.saturating_sub(4));
        let popup_width = 30.min(area.width.saturating_sub(4));
        let popup_area = Rect::new(layout[1].x + 1, layout[1].y + 2, popup_width, popup_height);

        let candidate_list =
            List::new(items).block(Block::default().borders(Borders::ALL).title("Candidates"));

        frame.render_widget(Clear, popup_area);
        frame.render_widget(candidate_list, popup_area);
    }

    // Status bar
    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(status, layout[2]);
}
