//! The fork's "animated" agent status indicators.
//!
//! In the animated style `status_icon` draws a private-use marker for each
//! animated state. Once the chrome is drawn, `StatusAnimation::resolve` swaps
//! the markers for the current frame, so the renderers need no animation
//! state, every icon on screen shows the same frame, and the fade can blend
//! toward the background the icon actually sits on.

use std::time::{Duration, Instant};

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};

use crate::api::schema::AgentStatus;
use crate::config::StatusIndicatorStyle;
use crate::protocol::FrameData;
use crate::terminal_theme::RgbColor;

/// The spinner shown for Working: a trail circling the middle 2×2 dots of
/// the braille cell. One turn takes 800 ms, and the blink phase always flips
/// on a spinner frame.
const SPINNER: [&str; 8] = ["⠒", "⠲", "⠰", "⠴", "⠤", "⠦", "⠆", "⠖"];
const SPINNER_FRAME_MS: u64 = 100;
/// Blocked and Done alternate between full and faded color this often.
const BLINK_PHASE_MS: u64 = 500;
/// The glyph Blocked and Done blink with, and stale states keep, as in "dots".
const DOT: &str = "●";

/// Markers from Supplementary Private Use Area-B, which nothing draws in the
/// chrome. They are one column wide, like the glyphs that replace them.
const SPINNER_MARKER: &str = "\u{10FF00}";
const BLINK_MARKER: &str = "\u{10FF01}";

/// What `status_icon` draws for `status` in the animated style.
pub(super) fn marker(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => SPINNER_MARKER,
        AgentStatus::Blocked | AgentStatus::Done => BLINK_MARKER,
        AgentStatus::Idle | AgentStatus::Unknown => super::status_dot(status),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Frame {
    spinner: usize,
    faded: bool,
}

/// The animation clock, and what the last composed frame put on screen.
pub(crate) struct StatusAnimation {
    epoch: Instant,
    drawn: Frame,
    /// Position of each animated icon in the last frame, and whether it spins.
    drawn_cells: Vec<(u16, u16, bool)>,
    spinner_on_screen: bool,
    blink_on_screen: bool,
}

impl StatusAnimation {
    pub(super) fn new(epoch: Instant) -> Self {
        Self {
            epoch,
            drawn: Frame::default(),
            drawn_cells: Vec::new(),
            spinner_on_screen: false,
            blink_on_screen: false,
        }
    }

    fn frame_at(&self, now: Instant) -> Frame {
        let elapsed = now.saturating_duration_since(self.epoch).as_millis();
        Frame {
            spinner: (elapsed / u128::from(SPINNER_FRAME_MS) % SPINNER.len() as u128) as usize,
            faded: elapsed / u128::from(BLINK_PHASE_MS) % 2 == 1,
        }
    }

    /// When the next frame of an animation on screen is due.
    pub(super) fn next_frame_at(&self, now: Instant) -> Option<Instant> {
        if !self.spinner_on_screen && !self.blink_on_screen {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.epoch).as_millis();
        let next_boundary = |period_ms: u64| {
            let period_ms = u128::from(period_ms);
            let boundary = u64::try_from((elapsed / period_ms + 1) * period_ms).ok()?;
            self.epoch.checked_add(Duration::from_millis(boundary))
        };
        let spinner = self
            .spinner_on_screen
            .then(|| next_boundary(SPINNER_FRAME_MS))
            .flatten();
        let blink = self
            .blink_on_screen
            .then(|| next_boundary(BLINK_PHASE_MS))
            .flatten();
        spinner.into_iter().chain(blink).min()
    }

    /// Whether an animation on screen shows a different frame at `now`.
    pub(super) fn frame_changed(&self, now: Instant) -> bool {
        if !self.spinner_on_screen && !self.blink_on_screen {
            return false;
        }
        let frame = self.frame_at(now);
        (self.spinner_on_screen && frame.spinner != self.drawn.spinner)
            || (self.blink_on_screen && frame.faded != self.drawn.faded)
    }

    /// Swaps the markers in `buffer` for the current frame and records which
    /// animations are on screen. `buffer` holds all visible status icons and no
    /// pane content; each call replaces what the previous one recorded.
    pub(super) fn resolve(
        &mut self,
        buffer: &mut Buffer,
        config: &super::ClientShellConfig,
        host_background: Option<RgbColor>,
    ) {
        self.resolve_at(
            buffer,
            config.status_indicators,
            host_background,
            Instant::now(),
        );
    }

    fn resolve_at(
        &mut self,
        buffer: &mut Buffer,
        style: StatusIndicatorStyle,
        host_background: Option<RgbColor>,
        now: Instant,
    ) {
        self.spinner_on_screen = false;
        self.blink_on_screen = false;
        self.drawn_cells.clear();
        if style != StatusIndicatorStyle::Animated {
            return;
        }
        let frame = self.frame_at(now);
        self.drawn = frame;
        let area = buffer.area;
        for (index, cell) in buffer.content.iter_mut().enumerate() {
            let spinner = match cell.symbol() {
                SPINNER_MARKER => true,
                BLINK_MARKER => false,
                _ => continue,
            };
            // Offline machines dim their rows; their last known states stay still.
            if cell.modifier.contains(Modifier::DIM) {
                cell.set_symbol(DOT);
                continue;
            }
            if spinner {
                cell.set_symbol(SPINNER[frame.spinner]);
                self.spinner_on_screen = true;
            } else {
                cell.set_symbol(DOT);
                if frame.faded {
                    fade(cell, host_background);
                }
                self.blink_on_screen = true;
            }
            let width = usize::from(area.width.max(1));
            let x = area.x.saturating_add((index % width) as u16);
            let y = area.y.saturating_add((index / width) as u16);
            self.drawn_cells.push((x, y, spinner));
        }
    }

    /// Keeps only the animations still visible in the finished `frame`, so
    /// icons hidden under an overlay or banner don't keep the clock running.
    pub(super) fn settle(&mut self, frame: &FrameData) {
        if self.drawn_cells.is_empty() {
            return;
        }
        let spinner_glyph = SPINNER[self.drawn.spinner];
        let visible = |x: u16, y: u16, glyph: &str| {
            x < frame.width
                && frame
                    .cells
                    .get(usize::from(y) * usize::from(frame.width) + usize::from(x))
                    .is_some_and(|cell| cell.symbol == glyph)
        };
        self.spinner_on_screen = self
            .drawn_cells
            .iter()
            .any(|&(x, y, spinner)| spinner && visible(x, y, spinner_glyph));
        self.blink_on_screen = self
            .drawn_cells
            .iter()
            .any(|&(x, y, spinner)| !spinner && visible(x, y, DOT));
    }
}

/// Blends the icon color halfway into the background behind it, so the icon
/// fades rather than darkens on light themes. Falls back to the terminal's
/// faint rendering when either color is not known as RGB.
fn fade(cell: &mut Cell, host_background: Option<RgbColor>) {
    let background = match cell.bg {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Reset => host_background.map(|color| (color.r, color.g, color.b)),
        _ => None,
    };
    match (cell.fg, background) {
        (Color::Rgb(r, g, b), Some((bg_r, bg_g, bg_b))) => {
            cell.fg = Color::Rgb(halfway(r, bg_r), halfway(g, bg_g), halfway(b, bg_b));
        }
        _ => cell.modifier.insert(Modifier::DIM),
    }
}

fn halfway(from: u8, to: u8) -> u8 {
    // At most 255, so the cast is lossless.
    (u16::from(from) + u16::from(to)).div_ceil(2) as u8
}

impl super::ClientShellState {
    /// Whether an animated status icon on screen is due for its next frame.
    pub(crate) fn tick_status_animation(&self, now: Instant) -> bool {
        self.status_animation.frame_changed(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn buffer_with(symbol: &str, style: Style) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(1, 0, symbol, style);
        buffer
    }

    #[test]
    fn markers_take_one_column_like_the_glyphs_that_replace_them() {
        use unicode_width::UnicodeWidthStr;
        for glyph in [SPINNER_MARKER, BLINK_MARKER, DOT].iter().chain(&SPINNER) {
            assert_eq!(glyph.width(), 1, "{glyph:?}");
        }
        assert_eq!(marker(AgentStatus::Idle), "○");
        assert_eq!(marker(AgentStatus::Unknown), "·");
    }

    #[test]
    fn spinner_and_blink_share_one_clock() {
        let epoch = Instant::now();
        let animation = StatusAnimation::new(epoch);
        for (elapsed, spinner, faded) in [
            (0, 0, false),
            (99, 0, false),
            (100, 1, false),
            (499, 4, false),
            // The blink flips on a spinner frame.
            (500, 5, true),
            (800, 0, true),
            (1000, 2, false),
        ] {
            assert_eq!(
                animation.frame_at(epoch + ms(elapsed)),
                Frame { spinner, faded },
                "{elapsed} ms"
            );
        }
    }

    #[test]
    fn next_frame_follows_only_the_animations_on_screen() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        assert_eq!(animation.next_frame_at(epoch + ms(10)), None);
        assert!(!animation.frame_changed(epoch + ms(10_000)));

        animation.blink_on_screen = true;
        assert_eq!(
            animation.next_frame_at(epoch + ms(10)),
            Some(epoch + ms(500))
        );
        assert_eq!(
            animation.next_frame_at(epoch + ms(500)),
            Some(epoch + ms(1000))
        );

        animation.spinner_on_screen = true;
        assert_eq!(
            animation.next_frame_at(epoch + ms(10)),
            Some(epoch + ms(100))
        );
        assert_eq!(
            animation.next_frame_at(epoch + ms(490)),
            Some(epoch + ms(500))
        );
    }

    #[test]
    fn frame_changes_only_when_a_drawn_animation_moves() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let mut buffer = buffer_with(BLINK_MARKER, Style::default().fg(Color::Rgb(200, 0, 0)));
        animation.resolve_at(&mut buffer, StatusIndicatorStyle::Animated, None, epoch);

        // A spinner frame passes without a blink phase change.
        assert!(!animation.frame_changed(epoch + ms(100)));
        assert!(animation.frame_changed(epoch + ms(500)));
    }

    #[test]
    fn resolve_draws_the_current_spinner_frame_in_the_working_color() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let working = Style::default().fg(Color::Rgb(249, 226, 175));
        let mut buffer = buffer_with(SPINNER_MARKER, working);

        animation.resolve_at(
            &mut buffer,
            StatusIndicatorStyle::Animated,
            None,
            epoch + ms(250),
        );

        assert_eq!(buffer[(1, 0)].symbol(), "⠰");
        assert_eq!(buffer[(1, 0)].fg, Color::Rgb(249, 226, 175));
        assert!(animation.spinner_on_screen);
        assert!(!animation.blink_on_screen);
    }

    #[test]
    fn faded_blink_blends_halfway_into_the_background_behind_it() {
        let epoch = Instant::now();
        let faded = epoch + ms(500);
        let red = Color::Rgb(200, 40, 60);

        // Dark row background.
        let mut animation = StatusAnimation::new(epoch);
        let mut dark = buffer_with(
            BLINK_MARKER,
            Style::default().fg(red).bg(Color::Rgb(0, 0, 0)),
        );
        animation.resolve_at(&mut dark, StatusIndicatorStyle::Animated, None, faded);
        assert_eq!(dark[(1, 0)].symbol(), DOT);
        assert_eq!(dark[(1, 0)].fg, Color::Rgb(100, 20, 30));

        // Terminal default background on a light terminal.
        let light_terminal = RgbColor {
            r: 255,
            g: 255,
            b: 255,
        };
        let mut light = buffer_with(BLINK_MARKER, Style::default().fg(red));
        animation.resolve_at(
            &mut light,
            StatusIndicatorStyle::Animated,
            Some(light_terminal),
            faded,
        );
        assert_eq!(light[(1, 0)].fg, Color::Rgb(228, 148, 158));
        assert!(!light[(1, 0)].modifier.contains(Modifier::DIM));

        // Full brightness half of the blink keeps the original color.
        let mut full = buffer_with(BLINK_MARKER, Style::default().fg(red));
        animation.resolve_at(
            &mut full,
            StatusIndicatorStyle::Animated,
            Some(light_terminal),
            epoch,
        );
        assert_eq!(full[(1, 0)].fg, red);
    }

    #[test]
    fn faded_blink_falls_back_to_faint_without_rgb_colors() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let mut named = buffer_with(BLINK_MARKER, Style::default().fg(Color::LightRed));
        animation.resolve_at(
            &mut named,
            StatusIndicatorStyle::Animated,
            None,
            epoch + ms(500),
        );
        assert_eq!(named[(1, 0)].fg, Color::LightRed);
        assert!(named[(1, 0)].modifier.contains(Modifier::DIM));

        let mut unknown_background =
            buffer_with(BLINK_MARKER, Style::default().fg(Color::Rgb(200, 40, 60)));
        animation.resolve_at(
            &mut unknown_background,
            StatusIndicatorStyle::Animated,
            None,
            epoch + ms(500),
        );
        assert_eq!(unknown_background[(1, 0)].fg, Color::Rgb(200, 40, 60));
        assert!(unknown_background[(1, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn dimmed_offline_icons_stay_still_and_do_not_keep_the_clock_running() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let stale = Style::default()
            .fg(Color::Rgb(108, 112, 134))
            .add_modifier(Modifier::DIM);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer.set_string(0, 0, SPINNER_MARKER, stale);
        buffer.set_string(1, 0, BLINK_MARKER, stale);

        animation.resolve_at(
            &mut buffer,
            StatusIndicatorStyle::Animated,
            None,
            epoch + ms(500),
        );

        assert_eq!(buffer[(0, 0)].symbol(), DOT);
        assert_eq!(buffer[(1, 0)].symbol(), DOT);
        assert_eq!(buffer[(1, 0)].fg, Color::Rgb(108, 112, 134));
        assert_eq!(animation.next_frame_at(epoch + ms(500)), None);
    }

    #[test]
    fn icons_covered_in_the_finished_frame_stop_the_clock() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(0, 0, SPINNER_MARKER, Style::default());
        buffer.set_string(2, 0, BLINK_MARKER, Style::default());
        animation.resolve_at(&mut buffer, StatusIndicatorStyle::Animated, None, epoch);
        let finished =
            |buffer: &Buffer| FrameData::from_ratatui_buffer_with_hyperlinks(buffer, None, &[]);

        animation.settle(&finished(&buffer));
        assert_eq!(animation.next_frame_at(epoch), Some(epoch + ms(100)));

        // An overlay covers the spinner; only the blink keeps the clock.
        buffer.set_string(0, 0, "x", Style::default());
        animation.settle(&finished(&buffer));
        assert_eq!(animation.next_frame_at(epoch), Some(epoch + ms(500)));

        buffer.set_string(2, 0, "x", Style::default());
        animation.settle(&finished(&buffer));
        assert_eq!(animation.next_frame_at(epoch), None);
        assert!(!animation.frame_changed(epoch + ms(1000)));
    }

    #[test]
    fn other_styles_skip_the_scan_and_stop_the_clock() {
        let epoch = Instant::now();
        let mut animation = StatusAnimation::new(epoch);
        let mut buffer = buffer_with(SPINNER_MARKER, Style::default());
        animation.resolve_at(&mut buffer, StatusIndicatorStyle::Animated, None, epoch);
        assert!(animation.next_frame_at(epoch).is_some());

        let mut buffer = buffer_with(SPINNER_MARKER, Style::default());
        animation.resolve_at(&mut buffer, StatusIndicatorStyle::Dots, None, epoch);
        assert_eq!(buffer[(1, 0)].symbol(), SPINNER_MARKER);
        assert_eq!(animation.next_frame_at(epoch), None);
        assert!(!animation.frame_changed(epoch + ms(1000)));
    }
}
