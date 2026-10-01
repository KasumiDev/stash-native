//! Provider-neutral chapter cards composed from the shared shelf widget.
use crate::stash::SceneMarker;
use crate::ui::card_row::{self, CardRow, RowStyle, TileLabel};
use crate::ui::machine::Measure;
use crate::ui::widgets::Art;
use crate::ui::{Painter, Rect};
use std::collections::HashMap;

pub(super) const SHIFT: f32 = 320.;
const TOP: f32 = 700.;
const STYLE: RowStyle = RowStyle {
    w: 288.,
    h: 162.,
    gap: 24.,
    ..RowStyle::EPISODE
};
#[derive(Default)]
pub(super) struct Markers {
    items: Vec<SceneMarker>,
    keys: Vec<u32>,
    motion: Option<CardRow>,
}
impl Markers {
    pub fn sync(&mut self, source: &[SceneMarker]) {
        let mut items: Vec<_> = source
            .iter()
            .filter(|m| m.seconds.is_finite() && m.seconds >= 0.)
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            a.seconds
                .total_cmp(&b.seconds)
                .then_with(|| a.id.cmp(&b.id))
        });
        if self.items.len() == items.len()
            && self.items.iter().zip(&items).all(|(a, b)| {
                a.id == b.id
                    && a.seconds == b.seconds
                    && a.title == b.title
                    && a.screenshot == b.screenshot
            })
        {
            return;
        }
        let old: HashMap<_, _> = self.items.iter().zip(&self.keys).map(|(m,k)| (m.id.clone(), *k)).collect();
        let mut reserved: Vec<u32> = items.iter().filter_map(|m| old.get(&m.id).copied()).collect();
        self.keys.clear();
        for item in &items {
            if let Some(key) = old.get(&item.id) { self.keys.push(*key); continue; }
            let hash = item
                .id
                .bytes()
                .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
            let mut key = 0x1000_0000 | (hash & 0x0fff_ffff);
            while reserved.contains(&key) {
                key = 0x1000_0000 | ((key + 1) & 0x0fff_ffff);
            }
            reserved.push(key);
            self.keys.push(key);
        }
        self.items = items;
    }
    pub fn keys(&self) -> &[u32] {
        &self.keys
    }
    pub fn contains(&self, key: u32) -> bool {
        self.keys.contains(&key)
    }
    pub fn empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn seconds(&self, key: u32) -> Option<f64> {
        self.keys
            .iter()
            .position(|k| *k == key)
            .map(|i| self.items[i].seconds)
    }
    pub fn near(&self, position: f64) -> Option<u32> {
        let i = self
            .items
            .iter()
            .rposition(|m| m.seconds <= position)
            .unwrap_or(0);
        self.keys.get(i).copied()
    }
    pub fn neighbour(&self, key: u32, right: bool) -> Option<u32> {
        let i = self.keys.iter().position(|k| *k == key)?;
        if right {
            self.keys.get(i + 1).copied()
        } else {
            i.checked_sub(1).map(|i| self.keys[i])
        }
    }
    pub fn update(&mut self, focus: Option<u32>, dt: f32) {
        let i = focus.and_then(|key| self.keys.iter().position(|k| *k == key));
        self.motion
            .get_or_insert_with(CardRow::new)
            .update(self.items.len(), i, &STYLE, dt);
    }
    pub fn rect(&self, key: u32, offset: f32) -> Option<Rect> {
        let i = self.keys.iter().position(|k| *k == key)?;
        let scroll = self.motion.as_ref().map_or(0., |m| m.scroll_x());
        let r = card_row::tile_rect(
            i,
            STYLE.margin_x,
            STYLE.w + STYLE.gap,
            scroll,
            TOP + SHIFT - offset,
            (STYLE.w, STYLE.h),
        );
        Some(r.scaled(self.motion.as_ref().map_or(1., |m| m.scale(i))))
    }
    pub fn images(&self, offset: f32) -> Vec<(String, String)> {
        self.items
            .iter()
            .zip(&self.keys)
            .filter(|(_, key)| {
                self.rect(**key, offset)
                    .is_some_and(|r| r.x + r.w >= 0. && r.x <= 1920.)
            })
            .filter_map(|(m, _)| {
                m.screenshot
                    .as_ref()
                    .map(|url| (format!("marker:{}", m.id), url.clone()))
            })
            .collect()
    }
    pub fn draw(
        &self,
        p: Painter,
        textures: &HashMap<String, (u32, f32, f32)>,
        focus: Option<u32>,
        offset: f32,
        measure: &dyn Measure,
    ) {
        let Some(row) = self.motion.as_ref() else {
            return;
        };
        let identities: Vec<_> = self
            .items
            .iter()
            .map(|m| format!("marker:{}", m.id))
            .collect();
        card_row::strip(
            p,
            row,
            self.items.len(),
            focus
                .and_then(|k| self.keys.iter().position(|x| *x == k))
                .map_or(-1, |i| i as i32),
            TOP + SHIFT - offset,
            (STYLE.w, STYLE.h),
            STYLE.w + STYLE.gap,
            &STYLE,
            1920.,
            |i| Art::Texture {
                key: &identities[i],
                image: textures.get(&identities[i]).copied(),
                portrait: false,
            },
            |_| None,
            |i| {
                TileLabel::titled(
                    &self.items[i].title,
                    &format!(
                        "{}:{:02}",
                        self.items[i].seconds as u64 / 60,
                        self.items[i].seconds as u64 % 60
                    ),
                )
            },
            |_, _, _, _| {},
            measure,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn marker(id: &str, seconds: f64) -> SceneMarker {
        SceneMarker {
            id: id.into(),
            seconds,
            title: id.into(),
            ..Default::default()
        }
    }
    #[test]
    fn markers_sort_and_keep_identity_when_earlier_marker_arrives() {
        let mut row = Markers::default();
        row.sync(&[marker("b", 20.), marker("a", 20.)]);
        assert_eq!(row.items[0].id, "a");
        let key = row.keys()[1];
        row.sync(&[marker("c", 0.), marker("b", 20.), marker("a", 20.)]);
        assert_eq!(row.seconds(key), Some(20.));
        assert_eq!(row.items[2].id, "b");
    }
    #[test]
    fn invalid_timestamps_are_never_seek_targets() {
        let mut row = Markers::default();
        row.sync(&[marker("a", f64::NAN), marker("b", -1.), marker("c", 10.)]);
        assert_eq!(row.keys().len(), 1);
        assert_eq!(row.seconds(row.near(0.).unwrap()), Some(10.));
    }
    #[test]
    fn reveal_geometry_moves_cards_above_screen_floor() {
        let mut row = Markers::default();
        row.sync(&[marker("a", 0.)]);
        let key = row.keys()[0];
        assert!(row.rect(key, 0.).unwrap().y > 1000.);
        assert!(row.rect(key, SHIFT).unwrap().y + 162. < 1080.);
    }
}
