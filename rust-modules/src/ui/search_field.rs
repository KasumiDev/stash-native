//! Provider-neutral Search field presentation extracted from PlxNative Search.
use crate::ui::label::Label;
use crate::ui::machine::Measure;
use crate::ui::{theme, Painter, Rect};
use std::ffi::CString;

pub(crate) const FIELD: Rect = Rect::new(
    crate::ui::consts::MARGIN_X,
    138.,
    crate::ui::consts::SCR_W - 2. * crate::ui::consts::MARGIN_X,
    80.,
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    p: Painter,
    rect: Rect,
    query: &str,
    caret: usize,
    editing: bool,
    phase_on: bool,
    hot: f32,
    measure: &dyn Measure,
) {
    let query = query.split('\0').next().unwrap_or("");
    let mut caret = caret.min(query.len());
    while !query.is_char_boundary(caret) {
        caret -= 1;
    }
    let blank = query.trim().is_empty();
    let run = if blank {
        crate::i18n::msg::browse_search_placeholder_c().to_owned()
    } else {
        CString::new(query).unwrap()
    };
    let head = CString::new(&query[..caret]).unwrap();
    let width = measure.width(&run, theme::size::HERO, true);
    let head_width = if blank {
        0.
    } else {
        measure.width(&head, theme::size::HERO, true)
    };
    let (offset, caret_x) = run_layout(width, head_width, rect.w, editing);
    let ink = if blank {
        theme::cross(theme::TEXT_TERTIARY, theme::TEXT_SECONDARY, hot)
    } else {
        theme::cross(
            theme::TEXT_SECONDARY,
            if editing {
                theme::FIELD_EDITING_INK
            } else {
                theme::FIELD_WAITING_INK
            },
            hot,
        )
    };
    let (cap_top, cap_base) = crate::text::text_cap_band(theme::size::HERO, 1);
    let pad = (crate::text::text_height(theme::size::HERO, 1)
        - rect.h * 0.5
        - (cap_top + cap_base) * 0.5)
        .max(0.);
    Label::new(run.as_ptr(), theme::size::HERO, ink)
        .bold()
        .draw(
            p.clipped(Rect::new(rect.x, rect.y, rect.w, rect.h + pad)),
            Rect::new(rect.x + offset, rect.y, rect.w, rect.h),
        );
    if editing && phase_on {
        let (cap_top, cap_base) = crate::text::text_cap_band(theme::size::HERO, 1);
        let y = crate::text::text_vcenter_y(theme::size::HERO, 1, rect.cy());
        p.rect(
            Rect::new(rect.x + caret_x, y + cap_top, 5., cap_base - cap_top),
            0.,
            theme::TEXT_PRIMARY,
            theme::TEXT_PRIMARY,
            0.,
        );
    }
}

pub(crate) fn run_layout(run: f32, caret: f32, width: f32, editing: bool) -> (f32, f32) {
    let available = (width - if editing { 13. } else { 0. }).max(0.);
    let overflow = (caret - available).max(0.).min((run - available).max(0.));
    (-overflow, (caret - overflow + 8.).min((width - 5.).max(0.)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_query_keeps_caret_in_field() {
        let (offset, caret) = run_layout(2000., 1800., 1000., true);
        assert!(offset < 0. && caret <= 995.);
    }
}
