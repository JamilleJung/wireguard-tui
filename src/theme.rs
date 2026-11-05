//! Display themes: default colours, high contrast, and an ASCII fallback with
//! no Unicode box drawing or symbols. `NO_COLOR` turns colours off in any
//! theme (state is then shown with bold/reverse/underline only).

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeName {
    #[default]
    Default,
    HighContrast,
    Ascii,
}

impl ThemeName {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "default" | "" => Some(Self::Default),
            "high-contrast" | "high_contrast" | "highcontrast" | "contrast" | "hc" => {
                Some(Self::HighContrast)
            }
            "ascii" | "plain" => Some(Self::Ascii),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::HighContrast => "high-contrast",
            Self::Ascii => "ascii",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: ThemeName,
    /// False under `NO_COLOR`.
    pub color: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: ThemeName::Default,
            color: true,
        }
    }
}

pub const THEME_NAMES: &str = "default, high-contrast, ascii";

impl Theme {
    /// `--theme` wins over `WG_TUI_THEME`; a non-empty `NO_COLOR` disables
    /// colour (https://no-color.org).
    pub fn resolve(
        cli: Option<&str>,
        env_theme: Option<&str>,
        no_color: Option<&str>,
    ) -> Result<Self, String> {
        let name = match cli.or(env_theme) {
            Some(value) => ThemeName::parse(value)
                .ok_or_else(|| format!("unknown theme '{value}' (choose one of: {THEME_NAMES})"))?,
            None => ThemeName::Default,
        };
        Ok(Self {
            name,
            color: no_color.is_none_or(str::is_empty),
        })
    }

    pub fn ascii(&self) -> bool {
        self.name == ThemeName::Ascii
    }

    fn high_contrast(&self) -> bool {
        self.name == ThemeName::HighContrast
    }

    fn fg(&self, normal: Color, contrast: Color, mono: Modifier) -> Style {
        if !self.color {
            Style::default().add_modifier(mono)
        } else if self.high_contrast() {
            Style::default().fg(contrast).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(normal)
        }
    }

    /// Field labels and other secondary text.
    pub fn label(&self) -> Style {
        self.fg(Color::DarkGray, Color::White, Modifier::empty())
    }
    /// Hints in the footer.
    pub fn hint(&self) -> Style {
        self.fg(Color::DarkGray, Color::White, Modifier::empty())
    }
    pub fn ok(&self) -> Style {
        self.fg(Color::Green, Color::LightGreen, Modifier::BOLD)
    }
    pub fn warn(&self) -> Style {
        self.fg(Color::Yellow, Color::LightYellow, Modifier::BOLD)
    }
    pub fn error(&self) -> Style {
        self.fg(
            Color::Red,
            Color::LightRed,
            Modifier::BOLD | Modifier::UNDERLINED,
        )
    }
    pub fn accent(&self) -> Style {
        self.fg(Color::Cyan, Color::LightCyan, Modifier::BOLD)
    }
    pub fn heading(&self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }
    pub fn plain(&self) -> Style {
        if self.color && self.high_contrast() {
            Style::default().fg(Color::White)
        } else if self.color {
            Style::default().fg(Color::Gray)
        } else {
            Style::default()
        }
    }
    /// The selected row in a list.
    pub fn selection(&self) -> Style {
        if !self.color {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else if self.high_contrast() {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        }
    }
    /// The focused field in the detail pane.
    pub fn focus(&self) -> Style {
        if !self.color || self.high_contrast() {
            self.selection()
        } else {
            Style::default().bg(Color::Blue)
        }
    }
    /// The highlighted tab.
    pub fn tab(&self) -> Style {
        if !self.color {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else if self.high_contrast() {
            self.selection()
        } else {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        }
    }
    /// The cursor line in the Log tab.
    pub fn cursor_line(&self) -> Style {
        if !self.color || self.high_contrast() {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        }
    }
    pub fn input_cursor(&self) -> Style {
        self.accent()
    }

    pub fn border_set(&self) -> border::Set<'static> {
        if self.ascii() {
            border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            }
        } else {
            border::PLAIN
        }
    }

    pub fn block(&self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_set(self.border_set())
    }

    pub fn tab_divider(&self) -> &'static str {
        if self.ascii() { "|" } else { "│" }
    }

    /// The marker before active / inactive tunnels.
    pub fn marker(&self, active: bool) -> &'static str {
        if active { "*" } else { "-" }
    }

    /// Replace the Unicode punctuation used in messages with ASCII when the
    /// ASCII theme is active.
    pub fn text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if !self.ascii() || text.is_ascii() {
            return Cow::Borrowed(text);
        }
        Cow::Owned(text.chars().map(ascii_for).collect())
    }

    /// Last line of defence for the ASCII theme: replace any non-ASCII symbol
    /// left in the frame (log text, file names) with a plain stand-in.
    pub fn finish(&self, buffer: &mut Buffer) {
        if !self.ascii() {
            return;
        }
        for cell in buffer.content.iter_mut() {
            let symbol = cell.symbol();
            if !symbol.is_ascii() {
                let replacement = symbol.chars().next().map(ascii_char).unwrap_or('?');
                cell.set_char(replacement);
            }
        }
    }
}

/// Multi-character ASCII stand-ins for message text.
fn ascii_for(c: char) -> String {
    match c {
        '…' => "...".into(),
        '—' | '–' => "-".into(),
        '→' => "->".into(),
        '←' => "<-".into(),
        '↓' => "v".into(),
        '↑' => "^".into(),
        '·' | '•' => "-".into(),
        _ if c.is_ascii() => c.to_string(),
        _ => ascii_char(c).to_string(),
    }
}

/// Single-cell ASCII stand-ins (keeps the layout width).
fn ascii_char(c: char) -> char {
    match c {
        '─' | '━' | '═' | '—' | '–' | '·' | '•' => '-',
        '│' | '┃' | '║' => '|',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╰' | '╯' | '╔' | '╗'
        | '╚' | '╝' => '+',
        '…' => '.',
        '↓' => 'v',
        '↑' => '^',
        '←' => '<',
        '→' => '>',
        '▀' | '▄' | '█' => '#',
        c if c.is_ascii() => c,
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_selection_prefers_the_flag_and_honours_no_color() {
        assert_eq!(Theme::resolve(None, None, None).unwrap(), Theme::default());
        let flag = Theme::resolve(Some("ascii"), Some("high-contrast"), None).unwrap();
        assert_eq!(flag.name, ThemeName::Ascii);
        let env = Theme::resolve(None, Some("HC"), None).unwrap();
        assert_eq!(env.name, ThemeName::HighContrast);
        assert!(env.color);
        assert!(!Theme::resolve(None, None, Some("1")).unwrap().color);
        // An empty NO_COLOR does not disable colour (per no-color.org).
        assert!(Theme::resolve(None, None, Some("")).unwrap().color);
        let error = Theme::resolve(Some("neon"), None, None).unwrap_err();
        assert!(error.contains("default, high-contrast, ascii"));
    }

    #[test]
    fn no_color_styles_carry_no_colours_but_keep_state_visible() {
        for name in [
            ThemeName::Default,
            ThemeName::HighContrast,
            ThemeName::Ascii,
        ] {
            let theme = Theme { name, color: false };
            for style in [
                theme.label(),
                theme.ok(),
                theme.warn(),
                theme.error(),
                theme.accent(),
                theme.selection(),
                theme.focus(),
                theme.tab(),
                theme.cursor_line(),
                theme.plain(),
            ] {
                assert_eq!(style.fg, None, "{name:?}");
                assert_eq!(style.bg, None, "{name:?}");
            }
            assert!(theme.selection().add_modifier.contains(Modifier::REVERSED));
        }
    }

    #[test]
    fn ascii_text_and_buffers_contain_only_ascii() {
        let theme = Theme {
            name: ThemeName::Ascii,
            color: true,
        };
        assert_eq!(
            theme.text("Reading wg0…  ↓ 1 B/s — ok"),
            "Reading wg0...  v 1 B/s - ok"
        );
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, 6, 1));
        buffer.set_string(0, 0, "│─…é▀", Style::default());
        theme.finish(&mut buffer);
        assert!(buffer.content.iter().all(|c| c.symbol().is_ascii()));
        let set = theme.border_set();
        for symbol in [set.top_left, set.horizontal_top, set.vertical_left] {
            assert!(symbol.is_ascii());
        }
    }
}
