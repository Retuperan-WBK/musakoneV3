//! Crossterm terminal operations with classic ANSI red for named UI colours.
//! Apple Terminal can render indexed red (38;5;1) differently from SGR 31.
//! Keep explicit indexed/RGB colours intact: artwork uses those intentionally.
use crossterm::cursor::MoveTo;
use crossterm::style::{
    Attribute, Attributes, Colored, Colors, Print, SetAttribute, SetAttributes, SetColors,
};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
use crossterm::{execute, queue};
use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::io::{self, Stdout, Write};

pub struct AnsiBackend<W: Write>(CrosstermBackend<W>);

impl<W: Write> AnsiBackend<W> {
    fn new(writer: W) -> Self {
        Self(CrosstermBackend::new(writer))
    }
}

pub fn init() -> io::Result<Terminal<AnsiBackend<Stdout>>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        hook(info);
    }));
    let result = (|| {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Terminal::new(AnsiBackend::new(io::stdout()))
    })();
    if result.is_err() {
        ratatui::restore();
    }
    result
}

fn attributes(modifier: Modifier) -> Attributes {
    let mut attributes = Attributes::default();
    for (flag, attribute) in [
        (Modifier::BOLD, Attribute::Bold),
        (Modifier::DIM, Attribute::Dim),
        (Modifier::ITALIC, Attribute::Italic),
        (Modifier::UNDERLINED, Attribute::Underlined),
        (Modifier::SLOW_BLINK, Attribute::SlowBlink),
        (Modifier::RAPID_BLINK, Attribute::RapidBlink),
        (Modifier::REVERSED, Attribute::Reverse),
        (Modifier::HIDDEN, Attribute::Hidden),
        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
    ] {
        if modifier.contains(flag) {
            attributes.set(attribute);
        }
    }
    attributes
}

impl<W: Write> Backend for AnsiBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut modifier = Modifier::empty();
        let mut last: Option<(u16, u16)> = None;
        queue!(self.0, SetAttribute(Attribute::Reset))?;
        for (x, y, cell) in content {
            if !matches!(last, Some((px, py)) if px.checked_add(1) == Some(x) && py == y) {
                queue!(self.0, MoveTo(x, y))?;
            }
            last = Some((x, y));
            if modifier != cell.modifier {
                // Reset/reapply attributes together so bold, dim and reverse never
                // leak into the next run. Reset also invalidates cached colours.
                queue!(
                    self.0,
                    SetAttribute(Attribute::Reset),
                    SetAttributes(attributes(cell.modifier))
                )?;
                modifier = cell.modifier;
                fg = Color::Reset;
                bg = Color::Reset;
            }
            if fg != cell.fg || bg != cell.bg {
                if cell.fg == Color::Red && !Colored::ansi_color_disabled_memoized() {
                    write!(
                        self.0,
                        "\x1b[31;{}m",
                        Colored::BackgroundColor(cell.bg.into())
                    )?;
                } else {
                    queue!(
                        self.0,
                        SetColors(Colors::new(cell.fg.into(), cell.bg.into()))
                    )?;
                }
                fg = cell.fg;
                bg = cell.bg;
            }
            queue!(self.0, Print(cell.symbol()))?;
        }
        queue!(self.0, SetAttribute(Attribute::Reset))
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.0.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.0.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.0.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.0.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.0.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.0.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.0.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.0.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.0.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn render(styles: &[Style]) -> String {
        let cells: Vec<_> = styles
            .iter()
            .map(|style| {
                let mut cell = Cell::new("X");
                cell.set_style(*style);
                cell
            })
            .collect();
        let mut bytes = Vec::new();
        let mut backend = AnsiBackend::new(&mut bytes);
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(x, cell)| (x as u16, 0, cell)),
            )
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn ui_red_uses_classic_ansi_but_artwork_retains_indexed_and_rgb_colours() {
        let output = render(&[
            Style::new().fg(Color::Red),
            Style::new().fg(Color::Indexed(1)).bg(Color::Indexed(4)),
            Style::new()
                .fg(Color::Rgb(170, 0, 0))
                .bg(Color::Rgb(0, 0, 170)),
            Style::new().fg(Color::White),
            Style::new().fg(Color::Yellow),
            Style::new().fg(Color::Red),
        ]);
        assert_eq!(output.matches("\x1b[31;49m").count(), 2);
        assert!(output.contains("\x1b[38;5;1;48;5;4mX"));
        assert!(output.contains("\x1b[38;2;170;0;0;48;2;0;0;170mX"));
        assert!(output.contains("\x1b[38;5;15;49mX"));
        assert!(output.contains("\x1b[38;5;3;49mX"));
    }

    #[test]
    fn style_transitions_restore_red_and_leave_no_dim_or_reverse_state() {
        let red = Style::new().fg(Color::Red);
        let output = render(&[
            red.add_modifier(Modifier::BOLD | Modifier::REVERSED),
            red,
            red.add_modifier(Modifier::DIM),
            red,
            Style::new(),
        ]);
        assert!(output.contains("\x1b[1m\x1b[7m\x1b[31;49mX"));
        assert!(output.contains("\x1b[2m\x1b[31;49mX"));
        assert_eq!(output.matches("X\x1b[0m\x1b[31;49mX").count(), 2);
        assert!(output.ends_with("\x1b[39;49mX\x1b[0m"));
        assert!(!output.contains("38;5;1m"));
    }
}
