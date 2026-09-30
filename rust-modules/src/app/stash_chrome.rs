//! Application-owned Stash vocabulary over the original shared top menu.
use crate::screens::stash_registry::{strip_destination, StashArg, StashHost};
use crate::ui::containers::tabs::StripMember;
use crate::ui::dispatch::{Dispatcher, STRIP_BASE};
use crate::ui::machine::{FocusKey, Measure};
use crate::ui::screen::ScreenArg;
use crate::ui::widgets::{self, ProfileChipRead, StripRender, TabLabels, TopFocus};
pub(crate) const SETTINGS: u32 = STRIP_BASE + 6;
pub(crate) struct StashChrome {
    labels: Vec<String>,
    keys: Vec<u32>,
    widths: Vec<f32>,
    strip: StripRender,
    initial: std::ffi::CString,
    name: std::ffi::CString,
    name_w: f32,
}
impl StashChrome {
    pub(crate) fn new(measure: &dyn Measure) -> Self {
        let labels = [
            crate::i18n::msg::browse_stash_home(),
            crate::i18n::msg::browse_stash_performers(),
            crate::i18n::msg::browse_stash_scenes(),
            crate::i18n::msg::browse_stash_galleries(),
            crate::i18n::msg::browse_stash_tags(),
            "",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let widths = widgets::tab_widths(&labels, measure);
        let (initial, name, name_w) =
            widgets::profile_chip_text(crate::i18n::msg::settings_title(), "S", measure);
        Self {
            labels,
            keys: (0..6).map(|i| STRIP_BASE + i).collect(),
            widths,
            strip: StripRender::new(),
            initial,
            name,
            name_w,
        }
    }
    fn focus(&self, key: Option<FocusKey<u32>>) -> TopFocus {
        match key.map(|k| k.elem) {
            Some(SETTINGS) => TopFocus::Chip,
            Some(k) => self
                .keys
                .iter()
                .position(|&v| v == k)
                .map_or(TopFocus::Away, TopFocus::Pill),
            None => TopFocus::Away,
        }
    }
    pub(crate) fn capture(&mut self, d: &mut Dispatcher<StashHost>, dt: f32) {
        let arg = d.top_arg().cloned().unwrap_or(StashArg::Home);
        if arg.chrome() != crate::ui::machine::Chrome::TabBar {
            d.nav.tabs.strip.clear();
            d.nav.tabs.strip_fallback = None;
            return;
        }
        let selected = (0..6)
            .find(|&i| strip_destination(STRIP_BASE + i) == Some(arg.clone()))
            .unwrap_or(0) as i32;
        let focus = self.focus(d.focus());
        let labels = TabLabels {
            generation: 0x53544153,
            labels: &self.labels,
        };
        self.strip.update(labels, selected, focus, dt);
        d.nav.tabs.strip.clear();
        d.nav
            .tabs
            .strip
            .push(StripMember::new(SETTINGS, widgets::CHIP_FRAME));
        widgets::tab_members(
            &self.widths,
            &self.keys,
            selected,
            focus,
            self.strip.scroll_pos(),
            &mut d.nav.tabs.strip,
        );
        d.nav.tabs.strip_fallback = Some(STRIP_BASE + selected as u32);
    }
    pub(crate) fn draw(
        &mut self,
        p: crate::ui::Painter,
        glass: &mut crate::ui::frame::glass::GlassPlan,
    ) {
        let labels = TabLabels {
            generation: 0x53544153,
            labels: &self.labels,
        };
        self.strip.draw(labels, p, glass.tab_band_mut());
        widgets::profile_chip_with(
            p,
            ProfileChipRead {
                thumb: "",
                initial: &self.initial,
                name: &self.name,
                name_w: self.name_w,
            },
            self.strip.chip_expand_pos(),
            glass.tab_face(),
        );
    }
}
