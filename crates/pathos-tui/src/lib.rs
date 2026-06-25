//! Pathos TUI — Ratatui terminal backend for interactive stories.
//!
//! Implements [`RenderBackend`] using Ratatui + crossterm.
//! [`TuiBackend::run()`] drives the full game loop.

use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use pathos_core::{Choice, NarrativeRuntime, RenderCommand, StepResult, UserInput};
use pathos_render::RenderBackend;

/// A Ratatui-based terminal render backend for Pathos stories.
pub struct TuiBackend {
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
    text_lines: Vec<String>,
    choices: Vec<Choice>,
    in_stream: bool,
}

impl TuiBackend {
    /// Create a new TUI backend. Switches the terminal to raw mode
    /// and enters the alternate screen.
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal: Some(terminal),
            text_lines: Vec::new(),
            choices: Vec::new(),
            in_stream: false,
        })
    }

    /// Run the narrative engine interactively.
    pub fn run(&mut self, runtime: &mut NarrativeRuntime) -> io::Result<()> {
        let start = runtime.config.start.clone();
        if let Err(e) = runtime.navigate_to(&start) {
            self.text_lines
                .push(format!("Error navigating to '{}': {}", start, e));
            self.draw()?;
            self.wait_for_any_key()?;
            return Ok(());
        }

        loop {
            match runtime.step() {
                StepResult::Render(cmds) => {
                    self.text_lines.clear();
                    self.choices.clear();
                    self.in_stream = false;
                    self.apply_commands(cmds);
                    self.draw()?;

                    if !self.choices.is_empty() {
                        if let Some(idx) = self.wait_for_choice()? {
                            runtime.submit_input(UserInput::Choice(idx));
                        } else {
                            return Ok(());
                        }
                    }
                }
                StepResult::WaitingForChoice => {
                    if self.choices.is_empty() {
                        self.text_lines
                            .push("[Internal: WaitingForChoice with no choices]".into());
                        self.draw()?;
                    }
                    if let Some(idx) = self.wait_for_choice()? {
                        runtime.submit_input(UserInput::Choice(idx));
                    } else {
                        return Ok(());
                    }
                }
                StepResult::WaitingForInput { prompt, default } => {
                    self.text_lines.push(prompt.clone());
                    self.draw()?;
                    if let Some(input) = self.wait_for_input(default.as_deref())? {
                        runtime.submit_input(UserInput::Text(input));
                    } else {
                        return Ok(());
                    }
                }
                StepResult::WaitingForStream { .. } => {
                    runtime.end_stream(Err("LLM unavailable (Phase 1)".into()));
                }
                StepResult::Finished => {
                    self.text_lines.push(String::new());
                    self.text_lines.push("— The End —".into());
                    self.draw()?;
                    self.wait_for_any_key()?;
                    return Ok(());
                }
            }
        }
    }

    fn apply_commands(&mut self, commands: Vec<RenderCommand>) {
        for cmd in commands {
            match cmd {
                RenderCommand::Clear => {
                    self.text_lines.clear();
                    self.choices.clear();
                }
                RenderCommand::Text(s) => {
                    self.text_lines.push(s);
                }
                RenderCommand::StreamBegin => {
                    self.in_stream = true;
                }
                RenderCommand::StreamToken(t) => {
                    if self.in_stream {
                        if let Some(last) = self.text_lines.last_mut() {
                            last.push_str(&t);
                        } else {
                            self.text_lines.push(t);
                        }
                    } else {
                        self.text_lines.push(t);
                    }
                }
                RenderCommand::StreamEnd => {
                    self.in_stream = false;
                }
                RenderCommand::StreamFailed { fallback } => {
                    self.in_stream = false;
                    self.text_lines.push(fallback);
                }
                RenderCommand::Choice(choices) => {
                    self.choices = choices;
                }
                RenderCommand::Input { .. } => {}
                RenderCommand::Separator => {
                    self.text_lines.push(String::new());
                }
            }
        }
    }

    fn draw(&mut self) -> io::Result<()> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        let text = self.text_lines.clone();
        let choices = self.choices.clone();
        terminal.draw(|f| {
            let area = f.area();
            if choices.is_empty() {
                let p = Paragraph::new(text.join("\n"))
                    .block(Block::default().borders(Borders::NONE))
                    .wrap(Wrap { trim: false });
                f.render_widget(p, area);
            } else {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),
                        Constraint::Length(2 + choices.len() as u16),
                    ])
                    .split(area);
                let p = Paragraph::new(text.join("\n"))
                    .block(Block::default().borders(Borders::NONE))
                    .wrap(Wrap { trim: false });
                f.render_widget(p, chunks[0]);

                let items: Vec<ListItem> = choices
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let prefix = if c.enabled {
                            format!("[{}]", i + 1)
                        } else {
                            "[x]".into()
                        };
                        ListItem::new(format!(" {} {}", prefix, c.label))
                    })
                    .collect();
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(" Choices "));
                f.render_widget(list, chunks[1]);
            }
        })?;
        Ok(())
    }

    fn wait_for_choice(&mut self) -> io::Result<Option<usize>> {
        loop {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let idx = (c as usize) - ('1' as usize);
                            if idx < self.choices.len() {
                                return Ok(Some(idx));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn wait_for_input(&mut self, default: Option<&str>) -> io::Result<Option<String>> {
        let mut input = default.unwrap_or("").to_string();
        loop {
            self.text_lines.push(format!("> {}", input));
            self.draw()?;
            self.text_lines.pop();
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Enter => return Ok(Some(input)),
                        KeyCode::Esc => return Ok(None),
                        KeyCode::Char(c) => input.push(c),
                        KeyCode::Backspace => {
                            input.pop();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn wait_for_any_key(&mut self) -> io::Result<()> {
        loop {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    return Ok(());
                }
            }
        }
    }
}

impl RenderBackend for TuiBackend {
    fn render(&mut self, commands: Vec<RenderCommand>) {
        self.apply_commands(commands);
    }

    fn clear(&mut self) {
        self.text_lines.clear();
        self.choices.clear();
    }
}

impl Drop for TuiBackend {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
