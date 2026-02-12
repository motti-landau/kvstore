use std::io::{self, stdout, Write};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::terminal::{self, ClearType};

use crate::store::{SearchScope, Store};
use crate::KvResult;

const POLL_INTERVAL: Duration = Duration::from_millis(120);
const KEY_PREVIEW_CHARS: usize = 56;
const VALUE_PREVIEW_CHARS: usize = 110;
const TAGS_PREVIEW_CHARS: usize = 56;

/// Runs an interactive fuzzy-search session that refreshes results as the user types.
pub fn live_search(storage: &Store, limit: usize, scope: SearchScope) -> KvResult<()> {
    let mut stdout = stdout();
    let guard = RawTerminalGuard::new()?;
    let mut input = String::new();
    let mut needs_render = true;
    let mut rendered_lines = 0usize;
    let mut first_draw = true;

    loop {
        if needs_render {
            if !first_draw {
                clear_previous(&mut stdout, rendered_lines)?;
            } else {
                first_draw = false;
            }
            rendered_lines = render(&mut stdout, storage, &input, limit, scope)?;
            needs_render = false;
        }

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if handle_key_event(key, &mut input) {
                    break;
                }
                needs_render = true;
            }
            Event::Resize(_, _) => {
                needs_render = true;
            }
            _ => {}
        }
    }

    drop(guard);
    Ok(())
}

fn handle_key_event(event: KeyEvent, input: &mut String) -> bool {
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        match event.code {
            KeyCode::Char('c') | KeyCode::Char('d') => return true,
            _ => return false,
        }
    }

    match event.code {
        KeyCode::Char(c) => {
            input.push(c);
        }
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Esc | KeyCode::Enter => return true,
        KeyCode::Delete => {
            input.clear();
        }
        _ => {}
    }
    false
}

fn render(
    stdout: &mut io::Stdout,
    storage: &Store,
    input: &str,
    limit: usize,
    scope: SearchScope,
) -> KvResult<usize> {
    let mut lines = 0usize;
    let max_line_chars = current_line_width_limit();

    let query_line = fit_for_terminal(&format!("Query: {input}"), max_line_chars);
    write_line(stdout, &query_line)?;
    lines += 1;

    if input.is_empty() {
        let message = fit_for_terminal("Type to search (Esc to exit).", max_line_chars);
        write_line(stdout, &message)?;
        lines += 1;
    } else {
        let matches = storage.search(input, limit, scope);
        if matches.is_empty() {
            let message = fit_for_terminal("No matches found.", max_line_chars);
            write_line(stdout, &message)?;
            lines += 1;
        } else {
            for entry in matches {
                let preview = preview_line(entry.key, entry.entry.value(), entry.entry.tags());
                let bounded_preview = fit_for_terminal(&preview, max_line_chars);
                write_line(stdout, &bounded_preview)?;
                lines += 1;
            }
        }
    }

    stdout.flush()?;
    Ok(lines)
}

fn clear_previous(stdout: &mut io::Stdout, lines: usize) -> KvResult<()> {
    if lines == 0 {
        return Ok(());
    }

    let mut remaining = lines;
    while remaining > 0 {
        let step = remaining.min(u16::MAX as usize) as u16;
        queue!(stdout, MoveUp(step))?;
        remaining -= step as usize;
    }
    queue!(stdout, MoveToColumn(0))?;
    queue!(stdout, terminal::Clear(ClearType::FromCursorDown))?;
    stdout.flush()?;
    Ok(())
}

fn write_line(stdout: &mut io::Stdout, text: &str) -> KvResult<()> {
    queue!(
        stdout,
        terminal::Clear(ClearType::CurrentLine),
        MoveToColumn(0)
    )?;
    writeln!(stdout, "{text}")?;
    Ok(())
}

fn preview_line(key: &str, value: &str, tags: &[String]) -> String {
    let key = truncate_for_display(&single_line(key), KEY_PREVIEW_CHARS);
    let value = if value.trim().is_empty() {
        "(empty)".to_string()
    } else {
        truncate_for_display(&single_line(value), VALUE_PREVIEW_CHARS)
    };

    if tags.is_empty() {
        format!("{key} = {value}")
    } else {
        let joined_tags = tags.join(", ");
        let tags = truncate_for_display(&single_line(&joined_tags), TAGS_PREVIEW_CHARS);
        format!("{key} = {value} [tags: {tags}]")
    }
}

fn current_line_width_limit() -> usize {
    terminal::size()
        .map(|(width, _)| width.saturating_sub(1) as usize)
        .unwrap_or(120)
}

fn fit_for_terminal(input: &str, max_chars: usize) -> String {
    truncate_for_display(&single_line(input), max_chars)
}

fn single_line(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_for_display(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(c) = chars.next() else {
            return input.to_string();
        };
        output.push(c);
    }

    if chars.next().is_some() {
        format!("{output}...")
    } else {
        output
    }
}

struct RawTerminalGuard;

impl RawTerminalGuard {
    fn new() -> KvResult<Self> {
        terminal::enable_raw_mode()?;
        let mut out = stdout();
        queue!(out, Hide)?;
        out.flush()?;
        Ok(Self)
    }
}

impl Drop for RawTerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut out = stdout();
        let _ = queue!(out, Show);
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_for_terminal, preview_line};

    #[test]
    fn fit_for_terminal_single_lines_and_truncates() {
        let output = fit_for_terminal("abc   def\nghi", 9);
        assert_eq!(output, "abc def g...");
    }

    #[test]
    fn preview_line_truncates_long_key() {
        let key = "this_is_a_very_long_key_name_that_should_be_trimmed_for_terminal_display";
        let line = preview_line(key, "value", &[]);
        assert!(line.contains("... = value"));
    }
}
