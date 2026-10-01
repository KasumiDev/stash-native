//! Measured hero typography, passive metadata chips, and a progressively pinned title.
#[cfg(test)]
use super::consts::MARGIN_X;
use super::consts::SCR_W;
use super::machine::Measure;
use super::text_view::TextView;
use super::widgets::{badge, badge_w, BadgeStyle, BADGE_H};
use super::{theme, Painter, Rect};

pub(crate) struct SceneTitle<'a> {
    first: String,
    second: Option<String>,
    measure: &'a dyn Measure,
}
pub(crate) fn scene_title<'a>(text: &str, measure: &'a dyn Measure) -> (SceneTitle<'a>, f32) {
    let text = text
        .split(|c: char| c.is_whitespace() && c != '\u{a0}')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let half = SCR_W * 0.5;
    let width = |s: &str| measure.width_str(s, theme::size::HERO, true);
    if width(&text) <= half {
        return (
            SceneTitle {
                first: text,
                second: None,
                measure,
            },
            half,
        );
    }
    let mut boundary = 0;
    for (at, c) in text.char_indices() {
        if c == ' ' {
            if width(&text[..at]) <= half {
                boundary = at;
            } else {
                break;
            }
        }
    }
    if boundary == 0 {
        // A single oversized word has no word boundary; retain a UTF-8-safe fitting prefix.
        for (at, c) in text.char_indices() {
            let end = at + c.len_utf8();
            if width(&text[..end]) > half {
                break;
            }
            boundary = end;
        }
    }
    let first = text[..boundary].trim_end().to_owned();
    let rest = text[boundary..].trim_start();
    let second = crate::text::elide_by(rest, SCR_W * 0.65, true, width);
    (
        SceneTitle {
            first,
            second: Some(second),
            measure,
        },
        SCR_W * 0.65,
    )
}
impl SceneTitle<'_> {
    fn line<'a>(&'a self, text: &'a str) -> TextView<'a> {
        TextView::new(text, theme::size::HERO, theme::TEXT_PRIMARY)
            .bold()
            .with_measure(self.measure)
            .max_lines(1)
    }
    pub(crate) fn measure_h(&self, _width: f32) -> f32 {
        self.line(&self.first).measure_h(SCR_W * 0.5)
            + self
                .second
                .as_ref()
                .map_or(0., |line| self.line(line).measure_h(SCR_W * 0.65))
    }
    pub(crate) fn draw(&self, p: Painter, frame: Rect) {
        let first = self.line(&self.first);
        first.draw(p, Rect::new(frame.x, frame.y, SCR_W * 0.5, 0.));
        if let Some(second) = &self.second {
            self.line(second).draw(
                p,
                Rect::new(
                    frame.x,
                    frame.y + first.measure_h(SCR_W * 0.5),
                    SCR_W * 0.65,
                    0.,
                ),
            );
        }
    }
}

pub(crate) struct PassiveBadge {
    pub rect: Rect,
    pub label: String,
}
pub(crate) struct PassiveBadges {
    pub items: Vec<PassiveBadge>,
    #[cfg(test)]
    pub hidden: usize,
}
impl PassiveBadges {
    pub(crate) fn new(labels: &[&str], width: f32, measure: &dyn Measure) -> Self {
        let mut items = Vec::new();
        let mut x = 0.;
        let mut row = 0;
        for label in labels {
            let text = crate::text::elide_by(label, (width - 24.).max(0.), false, |s| {
                measure.width_str(s, theme::size::CAPTION, true)
            });
            let w = badge_w(&text, None, measure).min(width);
            if x + w > width {
                row += 1;
                x = 0.;
            }
            if row >= 2 {
                break;
            }
            items.push(PassiveBadge {
                rect: Rect::new(x, row as f32 * (BADGE_H + theme::space::SM), w, BADGE_H),
                label: text,
            });
            x += w + theme::space::SM;
        }
        let mut hidden = labels.len() - items.len();
        if hidden > 0 {
            loop {
                let label = format!("+{hidden}");
                let w = badge_w(&label, None, measure).min(width);
                let (x, y) = items.last().map_or((0., 0.), |last| {
                    (last.rect.x + last.rect.w + theme::space::SM, last.rect.y)
                });
                if x + w <= width || items.is_empty() {
                    items.push(PassiveBadge {
                        rect: Rect::new(x, y, w, BADGE_H),
                        label,
                    });
                    break;
                }
                items.pop();
                hidden += 1;
            }
        }
        Self {
            items,
            #[cfg(test)]
            hidden,
        }
    }
    pub(crate) fn height(&self) -> f32 {
        self.items
            .last()
            .map_or(0., |item| item.rect.y + item.rect.h)
    }
    pub(crate) fn draw(&self, p: Painter, x: f32, y: f32, measure: &dyn Measure) {
        for item in &self.items {
            let rect = Rect::new(x + item.rect.x, y + item.rect.y, item.rect.w, item.rect.h);
            badge(
                p,
                rect.x,
                rect.cy(),
                &item.label,
                None,
                BadgeStyle::Translucent,
                measure,
            );
        }
    }
}

/// Header visibility begins only after the complete document hero passes below-navigation.
pub(crate) fn collapse_fraction(scroll: f32, extent: f32) -> f32 {
    ((scroll - extent) / theme::space::LG).clamp(0., 1.)
}
pub(crate) fn pinned_name_bottom(measure: &dyn Measure) -> f32 {
    super::widgets::TOP_BAR_BOTTOM
        + theme::space::MD
        + measure.line_h(theme::size::TITLE)
        + theme::space::XL
}
pub(crate) fn draw_pinned_name(p: Painter, text: &str, alpha: f32, measure: &dyn Measure) {
    if alpha <= 0. {
        return;
    }
    let p = p.alpha(alpha);
    let top = super::widgets::TOP_BAR_BOTTOM;
    let band = Rect::new(0., top, SCR_W, pinned_name_bottom(measure) - top);
    super::widgets::panel_ground(p, band, 0., None);
    TextView::new(text, theme::size::TITLE, theme::TEXT_PRIMARY)
        .bold()
        .with_measure(measure)
        .max_lines(1)
        .draw(
            p,
            Rect::new(
                super::consts::MARGIN_X,
                top + theme::space::MD,
                SCR_W - super::consts::MARGIN_X * 2.,
                0.,
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_title_first_line_stays_half_width_and_second_widens() {
        let _lock = crate::testlock::serial();
        let m = super::super::fixture::FixtureMeasure;
        let (_, short) = scene_title("A scene", &m);
        assert_eq!(short, SCR_W * 0.5);
        let long="An unusually long scene title describing many details that must wrap onto several lines before it can be displayed safely";
        let (title, wide) = scene_title(long, &m);
        assert_eq!(wide, SCR_W * 0.65);
        assert!(m.width_str(&title.first, theme::size::HERO, true) <= SCR_W * 0.5);
        assert!(
            m.width_str(title.second.as_ref().unwrap(), theme::size::HERO, true) <= SCR_W * 0.65
        );
        assert!(title.measure_h(wide) <= theme::size::HERO as f32 * 1.32 * 2.);
        assert!(MARGIN_X + wide < SCR_W - MARGIN_X);
    }
    #[test]
    fn passive_tags_have_at_most_two_rows_and_count_hidden_labels() {
        let m = super::super::fixture::FixtureMeasure;
        let labels = (0..30)
            .map(|i| format!("Tag with a long name {i}"))
            .collect::<Vec<_>>();
        let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
        let badges = PassiveBadges::new(&refs, 640., &m);
        assert!(badges.hidden > 0);
        assert!(badges.items.last().unwrap().label.starts_with('+'));
        assert_eq!(badges.items.len() - 1 + badges.hidden, labels.len());
        assert!(badges.height() <= BADGE_H * 2. + theme::space::SM);
        assert!(badges.items.iter().all(|b| b.rect.x + b.rect.w <= 640.));
    }
    #[test]
    fn performer_header_stays_hidden_until_the_entire_hero_leaves() {
        assert_eq!(collapse_fraction(100., 400.), 0.);
        assert_eq!(collapse_fraction(400., 400.), 0.);
        assert_eq!(collapse_fraction(450., 400.), 1.);
    }
    #[test]
    fn collapse_pins_the_name_and_reverses_with_scroll() {
        assert_eq!(collapse_fraction(0., 400.), 0.);
        assert_eq!(collapse_fraction(450., 400.), 1.);
        assert_eq!(collapse_fraction(0., 400.), 0.);
        assert_eq!(collapse_fraction(500., 400.), 1.);
    }
}
