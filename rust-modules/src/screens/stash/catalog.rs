//! Stash screens compose shared CardRow geometry; Input owns every cursor.
use crate::screens::stash_registry::*;
#[cfg(test)]
use crate::stash::Config;
use crate::stash::{Direction, Query};
use crate::stores::stash::{
    PageData, Section, ShelfId, ShelfPage, ShelfScope, ShelfSpec, Tile, Work,
};
use crate::ui::card_row::{self, CardRow, RowStyle, TileLabel};
use crate::ui::consts::{CARD_DY, GRID_TOP_Y, MARGIN_X, PEEK_Y, SCR_H, SCR_W, TITLE_DY};
use crate::ui::frame::Budget;
use crate::ui::geom::{Grid, Shelf};
use crate::ui::label::Label;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::theme;
use crate::ui::widgets::Art;
use crate::ui::{Painter, Rect, View};
use std::borrow::Cow;
use std::collections::HashMap;
#[derive(Default)]
struct State {
    generation: u32,
    provisional: bool,
    loading: bool,
    error: String,
    query: String,
    page: u32,
    sort: usize,
    scroll: f32,
    editing: u8,
    caret: usize,
    append: bool,
    media_at: u32,
    covered: bool,
    search_dirty_at: Option<u32>,
    refresh_pages: u32,
    loaded_pages: u32,
}
impl LogicalState for State {
    fn write(&self, c: &mut Canon) {
        c.u32(self.generation)
            .bool(self.provisional)
            .bool(self.loading)
            .str(&self.error)
            .str(&self.query)
            .u32(self.page)
            .u32(self.sort as u32)
            .f32(self.scroll)
            .u32(self.editing as u32)
            .u32(self.caret as u32)
            .bool(self.covered)
            .u32(self.refresh_pages)
            .u32(self.loaded_pages);
        c.option(self.search_dirty_at, |c, at| {
            c.u32(at);
        });
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!("page={} loading={}", self.page, self.loading));
    }
}
#[derive(Default)]
struct ShelfProgress {
    page: u32,
    loading: bool,
    exhausted: bool,
    error: String,
}

struct Row {
    pinned: bool,
    shelf_id: Option<ShelfId>,
    title: String,
    tiles: Vec<Tile>,
    keys: Vec<u32>,
    motion: CardRow,
    style: RowStyle,
    y: f32,
    collection: bool,
    ordinal: usize,
}
const CONTROLS_Y: f32 = GRID_TOP_Y - TITLE_DY;
const CONTENT_TOP: f32 = CONTROLS_Y + 64. + theme::space::XL + theme::space::MD;

pub struct StashScreen {
    route: StashArg,
    entry: EntryId,
    state: State,
    data: PageData,
    rows: Vec<Row>,
    keys: HashMap<String, u32>,
    next_key: u32,
    home_shelves: bool,
    detail_inset: Option<f32>,
    home_heading: f32,
    hit_top: f32,
    shelf_generation: u32,
    shelf_progress: HashMap<ShelfId, ShelfProgress>,
    scroll_motion: crate::ui::Spring,
    scroll_target: f32,
    bands: crate::ui::poster_grid::GridBands,
    media_images: Vec<(String, String)>,
    media_preview: Option<(String, String)>,
    refresh_data: Option<PageData>,
}
impl StashScreen {
    fn pinned(row: &Row) -> bool {
        row.pinned
    }
    fn row_y(&self, row: &Row) -> f32 {
        if Self::pinned(row) {
            row.y
        } else {
            row.y + crate::ui::poster_grid::growth_before(row.ordinal, &self.bands.geometry())
                - self.state.scroll
        }
    }
    #[cfg(test)]
    pub(crate) fn test_contains_content(&self, identity: &str) -> bool {
        self.data
            .sections
            .iter()
            .any(|s| s.tiles.iter().any(|t| t.identity == identity))
    }
    #[cfg(test)]
    pub(crate) fn test_focus_identity(&self, key: u32) -> Option<&str> {
        self.position(key)
            .map(|(r, c)| self.rows[r].tiles[c].identity.as_str())
    }
    #[cfg(test)]
    pub(crate) fn test_content_key(&self, identity: &str) -> Option<u32> {
        self.rows.iter().find_map(|row| {
            row.tiles
                .iter()
                .position(|tile| tile.identity == identity)
                .map(|index| row.keys[index])
        })
    }
    pub fn new(route: StashArg, entry: EntryId) -> Self {
        let provisional = matches!(route, StashArg::Tag(_));
        Self {
            route,
            entry,
            state: State {
                page: 1,
                provisional,
                ..Default::default()
            },
            data: PageData::default(),
            rows: Vec::new(),
            keys: HashMap::new(),
            next_key: 1,
            home_shelves: false,
            detail_inset: None,
            home_heading: PEEK_Y - TITLE_DY,
            hit_top: 0.,
            shelf_generation: 0,
            shelf_progress: HashMap::new(),
            scroll_motion: crate::ui::Spring::at(0.),
            scroll_target: 0.,
            bands: crate::ui::poster_grid::GridBands::new(),
            media_images: Vec::new(),
            media_preview: None,
            refresh_data: None,
        }
    }
    pub(crate) fn home(entry: EntryId) -> Self {
        let mut screen = Self::new(StashArg::Home, entry);
        screen.home_shelves = true;
        screen
    }
    pub(crate) fn performer(&self) -> Option<&crate::stash::Performer> {
        self.data.performer.as_ref()
    }
    pub(crate) fn reference_date(&self) -> Option<(i32, u32, u32)> {
        self.data.reference_date
    }
    pub(crate) fn media_override(
        &mut self,
        images: Vec<(String, String)>,
        preview: Option<(String, String)>,
    ) {
        self.media_images = images;
        self.media_preview = preview;
    }
    pub(crate) fn scenes(&self) -> &[crate::stash::Scene] {
        &self.data.scenes
    }
    pub(crate) fn loading(&self) -> bool {
        self.state.loading
    }
    pub(crate) fn retry(&mut self, fx: &mut Effects<'_, StashHost>) {
        if !self.state.loading {
            self.load(fx);
        }
    }
    pub(crate) fn shelf_scroll(&mut self, value: f32) {
        self.state.scroll = value;
        self.scroll_target = value;
        self.scroll_motion = crate::ui::Spring::at(value);
    }
    pub(crate) fn hit_clearance(&mut self, value: f32) {
        self.hit_top = value.max(0.).min(SCR_H);
    }
    fn hit_view(&self) -> Rect {
        let top = self.hit_top.max(self.content_view().y);
        Rect::new(0., top, SCR_W, SCR_H - top)
    }
    pub(crate) fn home_heading_top(&mut self, value: f32) {
        if (self.home_heading - value).abs() > 0.5 {
            self.home_heading = value;
            self.rebuild();
        }
    }
    pub(crate) fn home_scroll_target(&self, key: u32) -> Option<f32> {
        self.position(key).map(|(r, _)| {
            (self.rows[r].y - GRID_TOP_Y - CARD_DY).max(self.home_heading + TITLE_DY - GRID_TOP_Y)
        })
    }
    fn reveal_top(&self) -> f32 {
        if matches!(self.route, StashArg::Search) {
            300.
        } else if matches!(self.route, StashArg::Performer(_)) {
            255.
        } else {
            CONTENT_TOP
        }
    }
    fn content_view(&self) -> Rect {
        if self.home_shelves {
            let top = GRID_TOP_Y - TITLE_DY - theme::space::MD;
            Rect::new(0., top, SCR_W, SCR_H - top)
        } else if matches!(self.route, StashArg::Search) {
            Rect::new(0., 300., SCR_W, SCR_H - 300.)
        } else {
            Rect::FULL
        }
    }
    pub(crate) fn first_group(&self) -> Option<GroupId> {
        (!self.rows.is_empty()).then_some(GroupId(1))
    }
    pub(crate) fn scene(&self) -> Option<&crate::stash::Scene> {
        self.data.scene.as_ref()
    }
    pub(crate) fn detail(entry: EntryId, id: String) -> Self {
        let mut screen = Self::new(StashArg::Scene(id), entry);
        screen.detail_inset = Some(1000.);
        screen
    }
    pub(crate) fn detail_inset(&mut self, value: f32) {
        if self.detail_inset != Some(value) {
            self.detail_inset = Some(value);
            self.rebuild();
        }
    }
    pub(crate) fn scroll(&self) -> f32 {
        self.state.scroll
    }
    fn query(&self) -> Query {
        Query {
            q: self.state.query.clone(),
            page: self.state.page.max(1),
            per_page: 50,
            sort: match self.route {
                StashArg::Performers => "o_counter",
                StashArg::Tags => "name",
                _ => ["date", "title", "o_counter"][self.state.sort % 3],
            }
            .into(),
            direction: if self.state.sort % 3 == 1 || matches!(self.route, StashArg::Tags) {
                Direction::Ascending
            } else {
                Direction::Descending
            },
            ..Default::default()
        }
    }
    fn load(&mut self, fx: &mut Effects<'_, StashHost>) {
        self.state.generation = self.state.generation.wrapping_add(1);
        self.state.loading = true;
        if !self.state.append && self.state.page == 1 {
            self.shelf_generation = self.shelf_generation.wrapping_add(1);
            for progress in self.shelf_progress.values_mut() {
                progress.loading = false;
            }
        }
        self.state.error.clear();
        if matches!(self.route, StashArg::Search) && self.state.query.trim().is_empty() {
            self.state.loading = false;
            self.data = PageData::default();
            self.rebuild();
            return;
        }
        let query = self.query();
        fx.push(Fx::App(StashFx::Work(
            Addr {
                to: fx.from(),
                req: RequestId(self.state.generation),
            },
            Work::Load {
                route: self.route.clone(),
                query,
                generation: self.state.generation,
            },
        )));
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
    /// Merge provider pages without changing existing seats or accepting an endless
    /// duplicate-only page from a server whose count changed during browsing.
    fn land_page(&mut self, data: &PageData) {
        if !self.state.append {
            let retained = self
                .data
                .sections
                .iter()
                .filter(|section| {
                    section
                        .shelf_id
                        .as_ref()
                        .is_some_and(|id| data.shelves.iter().any(|spec| &spec.id == id))
                })
                .cloned()
                .collect::<Vec<_>>();
            self.data = data.clone();
            if matches!(self.route, StashArg::Performer(_) | StashArg::Tag(_)) {
                self.data.sections.splice(0..0, retained);
            } else {
                self.data.sections.extend(retained);
            }
            return;
        }
        append_page(&mut self.data, data);
    }
    fn refresh(&mut self, fx: &mut Effects<'_, StashHost>) {
        self.state.refresh_pages = self.state.loaded_pages.max(1);
        self.refresh_data = Some(PageData::default());
        self.state.page = 1;
        self.state.append = false;
        self.load(fx);
    }
    fn cancel_refresh(&mut self) {
        self.refresh_data = None;
        self.state.refresh_pages = 0;
    }
    fn land_refresh(&mut self, data: &PageData, fx: &mut Effects<'_, StashHost>) -> bool {
        let Some(pending) = self.refresh_data.as_mut() else {
            return true;
        };
        if self.state.page == 1 {
            *pending = data.clone();
        } else {
            append_page(pending, data);
        }
        if self.state.page < self.state.refresh_pages && data.has_more {
            self.state.page += 1;
            self.load(fx);
            return false;
        }
        let pending = self.refresh_data.take().unwrap();
        self.state.refresh_pages = 0;
        self.land_page(&pending);
        true
    }
    fn near_page_end(&self, focused: Option<u32>) -> bool {
        self.data
            .sections
            .iter()
            .filter(|section| {
                section.shelf_id.is_none()
                    && (!matches!(self.route, StashArg::Home) || section.title == "Performers")
            })
            .any(|section| {
                if section.tiles.is_empty() {
                    return false;
                }
                let focus_near = focused
                    .and_then(|key| self.position(key))
                    .and_then(|(r, c)| {
                        section
                            .tiles
                            .iter()
                            .position(|t| t.identity == self.rows[r].tiles[c].identity)
                    })
                    .is_some_and(|index| index + 12 >= section.tiles.len());
                if focus_near {
                    return true;
                }
                // Grid pages prefetch as their final row approaches the viewport. A
                // horizontal Home strip waits for its trailing cards to approach it.
                let Some(last) = section.tiles.last() else {
                    return false;
                };
                self.rows
                    .iter()
                    .find_map(|row| {
                        row.tiles
                            .iter()
                            .position(|t| t.identity == last.identity)
                            .map(|col| (row, col))
                    })
                    .is_some_and(|(row, col)| {
                        if matches!(self.route, StashArg::Home) {
                            let x = row.style.margin_x + col as f32 * (row.style.w + row.style.gap)
                                - row.motion.scroll_x();
                            x <= SCR_W + row.style.w * 2. && self.row_y(row) < SCR_H
                        } else {
                            self.row_y(row) <= SCR_H + row.style.h
                                && self.row_y(row) >= self.content_view().y
                        }
                    })
            })
    }
    fn prefetch(&mut self, focused: Option<u32>, fx: &mut Effects<'_, StashHost>) {
        if self.state.covered
            || self.state.loading
            || self.state.editing != 0
            || !self.state.error.is_empty()
            || !self.data.has_more
            || !self.near_page_end(focused)
        {
            return;
        }
        self.state.page = self.state.page.saturating_add(1);
        self.state.append = true;
        self.load(fx);
    }
    fn shelf_scope(&self) -> Option<ShelfScope> {
        match &self.route {
            StashArg::Home => Some(ShelfScope::Home),
            StashArg::Performer(id) => Some(ShelfScope::Performer(id.clone())),
            StashArg::Tag(id) => Some(ShelfScope::Tag(id.clone())),
            _ => None,
        }
    }
    fn request_shelf(&mut self, spec: ShelfSpec, fx: &mut Effects<'_, StashHost>) {
        let progress = self.shelf_progress.entry(spec.id.clone()).or_default();
        if progress.loading || progress.exhausted || self.state.covered {
            return;
        }
        progress.loading = true;
        progress.error.clear();
        fx.push(Fx::App(StashFx::Work(
            Addr {
                to: fx.from(),
                req: RequestId(self.shelf_generation),
            },
            Work::ShelfPage {
                spec,
                generation: self.shelf_generation,
                page: progress.page + 1,
            },
        )));
    }
    fn next_shelf(&mut self, fx: &mut Effects<'_, StashHost>) {
        if self.state.covered
            || self
                .shelf_progress
                .values()
                .any(|p| p.page == 0 && p.loading)
        {
            return;
        }
        if self
            .rows
            .iter()
            .filter(|r| r.shelf_id.is_some())
            .last()
            .is_some_and(|r| self.row_y(r) > SCR_H + r.style.h)
        {
            return;
        }
        let next = self
            .data
            .shelves
            .iter()
            .find(|spec| {
                self.shelf_progress
                    .get(&spec.id)
                    .is_none_or(|p| p.page == 0 && !p.loading && !p.exhausted)
            })
            .cloned();
        if let Some(spec) = next {
            self.request_shelf(spec, fx);
        }
    }
    fn prefetch_shelf(&mut self, key: u32, fx: &mut Effects<'_, StashHost>) {
        let Some((r, c)) = self.position(key) else {
            return;
        };
        let Some(id) = self.rows[r].shelf_id.clone() else {
            return;
        };
        if c + 12 < self.rows[r].tiles.len() {
            return;
        }
        if let Some(spec) = self.data.shelves.iter().find(|s| s.id == id).cloned() {
            self.request_shelf(spec, fx);
        }
    }
    fn land_shelf(
        &mut self,
        scope: &ShelfScope,
        id: &ShelfId,
        generation: u32,
        page: u32,
        result: &Result<ShelfPage, String>,
    ) -> bool {
        if generation != self.shelf_generation || self.shelf_scope().as_ref() != Some(scope) {
            return false;
        }
        let Some(progress) = self.shelf_progress.get_mut(id) else {
            return false;
        };
        if !progress.loading || page != progress.page + 1 {
            return false;
        }
        progress.loading = false;
        let response = match result {
            Ok(p) => p,
            Err(e) => {
                progress.error = e.clone();
                self.rebuild();
                return true;
            }
        };
        if response.section.shelf_id.as_ref() != Some(id) {
            progress.error = crate::stash::Error::InvalidResponse.to_string();
            self.rebuild();
            return true;
        }
        progress.page = page;
        progress.exhausted = !response.has_more
            || page as usize * crate::stores::stash::SHELF_PAGE_SIZE as usize >= response.count;
        if let Some(existing) = self
            .data
            .sections
            .iter_mut()
            .find(|s| s.shelf_id.as_ref() == Some(id))
        {
            let before = existing.tiles.len();
            for tile in &response.section.tiles {
                if !existing.tiles.iter().any(|t| t.identity == tile.identity) {
                    existing.tiles.push(tile.clone());
                }
            }
            if before == existing.tiles.len() {
                progress.exhausted = true;
            }
        } else if !response.section.tiles.is_empty() {
            let at = if matches!(self.route, StashArg::Home) {
                self.data.sections.len()
            } else {
                self.data
                    .sections
                    .iter()
                    .position(|s| !s.shelf)
                    .unwrap_or(self.data.sections.len())
            };
            self.data.sections.insert(at, response.section.clone());
        }
        let order = self
            .data
            .shelves
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        if matches!(self.route, StashArg::Home) {
            if self.data.sections.len() > 2 {
                self.data.sections[2..].sort_by_key(|s| {
                    s.shelf_id
                        .as_ref()
                        .and_then(|id| order.iter().position(|x| x == id))
                        .unwrap_or(usize::MAX)
                });
            }
        } else {
            self.data.sections.sort_by_key(|s| {
                s.shelf_id
                    .as_ref()
                    .and_then(|id| order.iter().position(|x| x == id))
                    .unwrap_or(usize::MAX)
            });
        }
        self.rebuild();
        true
    }
    fn control(id: &str, title: String, action: Action) -> Tile {
        Tile {
            scene_metadata: None,
            identity: format!("control:{id}"),
            title,
            o_count: None,
            caption: String::new(),
            image: None,
            preview: None,
            action,
        }
    }
    fn rebuild(&mut self) {
        let mut sections = Vec::new();
        let mut controls = Vec::new();
        if matches!(self.route, StashArg::Search) {
            sections.push(Section {
                shelf_id: None,
                title: String::new(),
                tiles: vec![Self::control(
                    "search",
                    self.state.query.clone(),
                    Action::EditSearch,
                )],
                portrait: false,
                shelf: false,
            });
        }
        if !self.state.error.is_empty() || self.shelf_progress.values().any(|p| !p.error.is_empty())
        {
            controls.push(Self::control("retry", "Retry".into(), Action::Refresh));
        }
        if matches!(self.route, StashArg::Scenes) {
            controls.push(Self::control(
                "sort",
                format!(
                    "Sort: {}",
                    ["Date", "Title", "O count"][self.state.sort % 3]
                ),
                Action::Sort,
            ));
        }
        if !self.home_shelves && !controls.is_empty() {
            sections.push(Section {
                shelf_id: None,
                title: String::new(),
                tiles: controls,
                portrait: false,
                shelf: false,
            });
        }
        sections.extend(
            self.data
                .sections
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    (!self.home_shelves || *i != 0)
                        && (self.detail_inset.is_none() || s.title != "Playback")
                })
                .map(|(_, s)| s.clone()),
        );
        let mut old_rows = std::mem::take(&mut self.rows);
        let mut y = if self.detail_inset.is_some()
            && self.data.scene.is_none()
            && self.data.performer.is_none()
        {
            CONTROLS_Y
        } else if let Some(inset) = self.detail_inset {
            inset
        } else if self.home_shelves {
            self.home_heading + TITLE_DY
        } else if sections.first().is_some_and(control_section) {
            CONTROLS_Y
        } else {
            CONTENT_TOP - CARD_DY
        };
        if matches!(self.route, StashArg::Search) {
            y = 300. - CARD_DY;
        }
        for (section_index, s) in sections.into_iter().enumerate() {
            let collection =
                !control_section(&s) && !matches!(self.route, StashArg::Home) && !s.shelf;
            let control = s
                .tiles
                .first()
                .is_some_and(|t| t.identity.starts_with("control:"));
            let (columns, collection_style) = crate::ui::poster_grid::collection_style(
                s.portrait,
                matches!(self.route, StashArg::Tags),
            );
            let style = if control {
                RowStyle {
                    w: if matches!(self.route, StashArg::Search) && section_index == 0 {
                        crate::ui::search_field::FIELD.w
                    } else {
                        225.
                    },
                    h: if matches!(self.route, StashArg::Search) {
                        80.
                    } else {
                        64.
                    },
                    gap: 20.,
                    margin_x: MARGIN_X,
                    focus_scale: if matches!(self.route, StashArg::Search) && section_index == 0 {
                        1.
                    } else {
                        RowStyle::EPISODE.focus_scale
                    },
                    ..RowStyle::EPISODE
                }
            } else if collection {
                collection_style
            } else if s.portrait {
                RowStyle::HOME
            } else {
                RowStyle::EPISODE
            };
            let chunk = if !collection {
                s.tiles.len().max(1)
            } else {
                columns
            };
            if matches!(self.route, StashArg::Search) && control {
                y = if section_index == 0 {
                    crate::ui::search_field::FIELD.y
                } else {
                    230.
                };
            }
            for (i, tiles) in s.tiles.chunks(chunk).enumerate() {
                if !control {
                    y += if i == 0 {
                        if self.home_shelves || s.title == self.data.title {
                            CARD_DY
                        } else {
                            TITLE_DY + CARD_DY
                        }
                    } else {
                        theme::space::MD
                    };
                }
                let keys: Vec<u32> = tiles
                    .iter()
                    .map(|t| {
                        let identity = if control {
                            t.identity.clone()
                        } else {
                            format!(
                                "{}{}:{}",
                                if s.shelf && !matches!(self.route, StashArg::Home) {
                                    "shelf:"
                                } else {
                                    ""
                                },
                                s.shelf_id
                                    .as_ref()
                                    .map(|id| format!("{id:?}"))
                                    .unwrap_or_else(|| s.title.clone()),
                                t.identity
                            )
                        };
                        let key = self.keys.entry(identity).or_insert_with(|| {
                            let k = self.next_key;
                            self.next_key += 1;
                            k
                        });
                        *key
                    })
                    .collect();
                let motion = old_rows
                    .iter()
                    .position(|old| {
                        if s.shelf_id.is_some() {
                            old.shelf_id == s.shelf_id
                        } else {
                            old.shelf_id.is_none() && old.keys.first() == keys.first()
                        }
                    })
                    .map(|at| old_rows.remove(at).motion)
                    .unwrap_or_else(CardRow::new);
                self.rows.push(Row {
                    pinned: control && self.detail_inset.is_none() && !self.home_shelves,
                    shelf_id: s.shelf_id.clone(),
                    title: if i == 0 && s.title != self.data.title {
                        s.title.clone()
                    } else {
                        String::new()
                    },
                    tiles: tiles.to_vec(),
                    keys,
                    motion,
                    style,
                    y,
                    collection,
                    ordinal: self.rows.len(),
                });
                y += style.h
                    + if control {
                        theme::space::MD
                    } else if collection {
                        card_row::LABEL_BAND_COLLAPSED + crate::ui::consts::UNDER_LABEL_AIR
                    } else {
                        TileLabel::height(true) + theme::space::XL
                    };
            }
            if control && self.detail_inset.is_none() {
                y = self.reveal_top() - CARD_DY;
            }
        }
    }
    fn grid<R>(&self, f: impl FnOnce(Grid<'_>) -> R) -> R {
        let shelves: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| Shelf {
                row: &row.motion,
                n: row.tiles.len(),
                sty: &row.style,
                row_y: self.row_y(row),
                size: (row.style.w, row.style.h),
                pitch: row.style.w + row.style.gap,
                group: GroupId(
                    i as u32
                        + if self.home_shelves || self.detail_inset.is_some() {
                            1
                        } else {
                            0
                        },
                ),
                entry: self.entry,
                extent: Rect::new(
                    MARGIN_X,
                    self.row_y(row),
                    SCR_W - 2. * MARGIN_X,
                    row.style.h,
                ),
            })
            .collect();
        f(Grid {
            shelves: &shelves,
            stride: 1000,
            entry: self.entry,
        })
    }
    fn position(&self, key: u32) -> Option<(usize, usize)> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.keys.iter().position(|k| *k == key).map(|c| (r, c)))
    }
    fn internal(&self, key: u32) -> u32 {
        self.position(key)
            .map_or(0, |(r, c)| r as u32 * 1000 + c as u32)
    }
    fn external(&self, key: u32) -> u32 {
        self.rows
            .get((key / 1000) as usize)
            .and_then(|r| r.keys.get((key % 1000) as usize))
            .copied()
            .unwrap_or(0)
    }
    fn media(&self, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        if self.state.covered {
            return;
        }
        let mut images = Vec::new();
        if self.detail_inset.is_some() {
            if let Some(scene) = self.scene() {
                if let Some(url) = &scene.paths.screenshot {
                    images.push((format!("scene:{}", scene.id), url.clone()));
                }
            }
        }
        if self.home_shelves {
            for scene in self.data.scenes.iter().take(12) {
                if let Some(url) = &scene.paths.screenshot {
                    images.push((format!("hero:{}", scene.id), url.clone()));
                }
            }
        }
        for row in &self.rows {
            if self.row_y(row) > 1080. || self.row_y(row) + row.style.h < self.content_view().y {
                continue;
            }
            for (index, tile) in row.tiles.iter().enumerate() {
                let Some(placed) = self.place(&row.keys[index], cx, At::Drawn) else {
                    continue;
                };
                if placed.rect.x + placed.rect.w < 0. || placed.rect.x > 1920. {
                    continue;
                }
                if let Some(url) = &tile.image {
                    images.push((tile.identity.clone(), url.clone()));
                    if let Some(metadata) = &tile.scene_metadata {
                        let ratio = cx
                            .views
                            .textures
                            .get(&format!("preview:{}", tile.identity))
                            .or_else(|| cx.views.textures.get(&tile.identity))
                            .filter(|(_, w, h)| *w > 0. && *h > 0.)
                            .map(|(_, w, h)| w / h)
                            .or(metadata.aspect_ratio);
                        if ratio.is_none_or(|a| !crate::ui::widgets::scene_aspect_is_standard(a)) {
                            images.push((format!("blur:{}", tile.identity), url.clone()));
                        }
                    }
                }
            }
        }
        images.extend(self.media_images.iter().cloned());
        let preview = self.media_preview.clone().or_else(|| {
            cx.focus
                .current
                .and_then(|k| self.position(k.elem))
                .and_then(|(r, c)| self.rows[r].tiles.get(c))
                .and_then(|t| {
                    t.preview
                        .as_ref()
                        .map(|url| (t.identity.clone(), url.clone()))
                })
        });
        fx.push(Fx::App(StashFx::Media(images, preview)));
    }
    fn activate(&mut self, key: u32, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        let Some((r, c)) = self.position(key) else {
            return;
        };
        let action = self.rows[r].tiles[c].action.clone();
        match action {
            Action::Open(route) => {
                fx.push(Fx::Nav(
                    if matches!(
                        route,
                        StashArg::Home
                            | StashArg::Scenes
                            | StashArg::Performers
                            | StashArg::Galleries
                            | StashArg::Tags
                            | StashArg::Search
                            | StashArg::Settings
                    ) {
                        NavOp::Root(route)
                    } else {
                        NavOp::Push(route)
                    },
                ));
            }
            Action::Play(scene, resume) => {
                fx.push(Fx::App(StashFx::Play(scene.clone(), resume)));
                fx.push(Fx::Nav(NavOp::Push(StashArg::Player(scene.id))));
            }
            Action::Refresh => {
                let failed = self
                    .data
                    .shelves
                    .iter()
                    .filter(|s| {
                        self.shelf_progress
                            .get(&s.id)
                            .is_some_and(|p| !p.error.is_empty())
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if !failed.is_empty() {
                    for spec in failed {
                        self.request_shelf(spec, fx);
                    }
                    self.rebuild();
                    return;
                }
                if self.state.loading {
                    return;
                }
                if self.refresh_data.is_some() || self.state.append {
                    self.load(fx);
                } else {
                    self.refresh(fx);
                }
            }
            Action::Sort => {
                self.cancel_refresh();
                self.state.sort = (self.state.sort + 1) % 3;
                self.state.page = 1;
                self.state.append = false;
                self.load(fx);
            }
            Action::EditSearch => {
                self.state.editing = 1;
                self.state.caret = self.state.query.len();
                fx.push(Fx::App(StashFx::Keyboard(true)));
            }
            _ => {}
        }
        self.rebuild();
        self.media(cx, fx);
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
}
impl Focusable<StashHost> for StashScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        self.grid(|g| g.groups(cx, out));
    }
    fn group_of(&self, key: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        self.position(*key)?;
        self.grid(|g| g.group_of(&self.internal(*key), cx))
    }
    fn neighbour(&self, k: FocusKey<u32>, d: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        if let Some((r, c)) = self.position(k.elem) {
            let next = match d {
                Dir::Up => r.checked_sub(1),
                Dir::Down => (r + 1 < self.rows.len()).then_some(r + 1),
                _ => None,
            };
            if let Some(next) = next {
                let target = &self.rows[next];
                let from_x = self
                    .place(&k.elem, cx, At::SpringTarget)
                    .map_or(target.style.margin_x + target.style.w * 0.5, |p| {
                        p.rest_rect.cx()
                    });
                let column = card_row::column_near_x(
                    from_x,
                    target.style.margin_x,
                    target.style.w + target.style.gap,
                    target.style.w,
                    target.motion.scroll_x(),
                    target.keys.len(),
                    c,
                );
                if let Some(elem) = target.keys.get(column) {
                    return Step::Move(FocusKey {
                        entry: self.entry,
                        elem: *elem,
                    });
                }
            }
        }
        self.grid(|g| {
            match g.neighbour(
                FocusKey {
                    entry: k.entry,
                    elem: self.internal(k.elem),
                },
                d,
                cx,
            ) {
                Step::Move(k) => Step::Move(FocusKey {
                    entry: k.entry,
                    elem: self.external(k.elem),
                }),
                Step::Edge => Step::Edge,
            }
        })
    }
    fn place(&self, k: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        let (r, _) = self.position(*k)?;
        self.grid(|g| g.place(&self.internal(*k), cx, at))
            .map(|mut place| {
                if !Self::pinned(&self.rows[r]) {
                    place.clip = place.clip.intersect(self.hit_view());
                }
                place
            })
    }
    fn reconcile(&self, k: FocusKey<u32>, _: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if self.position(k.elem).is_some() {
            k
        } else {
            FocusKey {
                entry: self.entry,
                elem: self
                    .rows
                    .first()
                    .and_then(|r| r.keys.first())
                    .copied()
                    .unwrap_or(0),
            }
        }
    }
    fn seat(&self, g: GroupId, from: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        self.grid(|grid| {
            let k = grid.seat(g, from, cx);
            FocusKey {
                entry: k.entry,
                elem: self.external(k.elem),
            }
        })
    }
}
impl Machine<StashHost> for StashScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        ev: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        if matches!(
            ev,
            ScreenEvent::Input(_) | ScreenEvent::Enter(Enter::Restored)
        ) || matches!(
            ev,
            ScreenEvent::FocusMoved {
                by: By::Dir | By::Pointer,
                ..
            }
        ) {
            self.state.provisional = false;
        }
        match ev {
            ScreenEvent::Mount => {
                self.rebuild();
                self.load(fx);
                if matches!(self.route, StashArg::Search) {
                    self.state.editing = 1;
                    fx.push(Fx::App(StashFx::Keyboard(true)));
                    if let Some(key) = self.keys.get("control:search") {
                        fx.push(Fx::Deliver(
                            fx.from(),
                            Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                                focus: FocusTarget::Elem(FocusKey {
                                    entry: self.entry,
                                    elem: *key,
                                }),
                            })),
                        ));
                    }
                }
                Handled::Yes
            }
            ScreenEvent::Uncover => {
                self.state.covered = false;
                if !self.state.loading {
                    self.refresh(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Async(_, StashMsg::Loaded { generation, result })
                if *generation == self.state.generation && self.state.loading =>
            {
                self.state.loading = false;
                match result {
                    Ok(data) => {
                        let old_focus = cx.focus.current.and_then(|key| {
                            self.position(key.elem)
                                .map(|(row, _)| (key.elem, self.rows[row].y))
                        });
                        if self.refresh_data.is_some() {
                            if !self.land_refresh(data, fx) {
                                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                                return Handled::Yes;
                            }
                        } else {
                            self.land_page(data);
                        }
                        self.state.loaded_pages = self.state.page;
                        if let Some(error) = &self.data.shelves_error {
                            self.state.error = error.clone();
                        }
                        self.state.append = false;
                        self.rebuild();
                        if let Some((key, old_y)) = old_focus {
                            if let Some((row, _)) = self.position(key) {
                                let shift = self.rows[row].y - old_y;
                                self.state.scroll += shift;
                                self.scroll_target += shift;
                                self.scroll_motion.pos += shift;
                            }
                        }
                        self.media(cx, fx);
                        self.next_shelf(fx);
                    }
                    Err(error) => {
                        self.state.error = error.clone();
                        self.rebuild();
                    }
                }
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                Handled::Yes
            }
            ScreenEvent::Async(
                _,
                StashMsg::ShelfPageLoaded {
                    scope,
                    id,
                    generation,
                    page,
                    result,
                },
            ) => {
                let old_focus = cx
                    .focus
                    .current
                    .and_then(|k| self.position(k.elem).map(|(r, _)| (k.elem, self.rows[r].y)));
                if self.land_shelf(scope, id, *generation, *page, result) {
                    if self.state.provisional && matches!(self.route, StashArg::Tag(_)) {
                        self.shelf_scroll(0.);
                        if let Some(key) =
                            self.rows.first().and_then(|row| row.keys.first()).copied()
                        {
                            fx.push(Fx::Deliver(
                                fx.from(),
                                Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                                    focus: FocusTarget::Elem(FocusKey {
                                        entry: self.entry,
                                        elem: key,
                                    }),
                                })),
                            ));
                        }
                    } else if let Some((key, y)) = old_focus {
                        if let Some((r, _)) = self.position(key) {
                            let shift = self.rows[r].y - y;
                            self.state.scroll += shift;
                            self.scroll_target += shift;
                            self.scroll_motion.pos += shift;
                        }
                    }
                    self.media(cx, fx);
                    if result.is_ok() {
                        self.next_shelf(fx);
                    }
                    fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                }
                Handled::Yes
            }
            ScreenEvent::Activate(key) => {
                if let Some(arg) = strip_destination(*key) {
                    fx.push(Fx::Nav(NavOp::Root(arg)));
                    return Handled::Yes;
                }
                self.activate(*key, cx, fx);
                Handled::Yes
            }
            ScreenEvent::PressCommit(_) => {
                if let Some(k) = cx.focus.current {
                    if let Some(arg) = strip_destination(k.elem) {
                        fx.push(Fx::Nav(NavOp::Root(arg)));
                        return Handled::Yes;
                    }
                    self.activate(k.elem, cx, fx);
                }
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } => {
                if matches!(self.route, StashArg::Search)
                    && self.state.editing != 0
                    && self.keys.get("control:search") != Some(&to.elem)
                {
                    self.state.editing = 0;
                    fx.push(Fx::App(StashFx::Keyboard(false)));
                }
                if let Some((r, _)) = self.position(to.elem) {
                    let row = &self.rows[r];
                    if !Self::pinned(row) && !self.home_shelves {
                        let max = self
                            .rows
                            .last()
                            .map_or(0., |last| {
                                last.y
                                    + crate::ui::poster_grid::growth_before(
                                        last.ordinal,
                                        &crate::ui::poster_grid::settled(Some(r)),
                                    )
                                    + last.style.h
                                    + TileLabel::height(true)
                                    - (SCR_H - crate::ui::consts::MARGIN_Y)
                            })
                            .max(0.);
                        self.scroll_target = card_row::reveal(
                            self.state.scroll,
                            row.y + row.style.h + TileLabel::height(true)
                                - (SCR_H - crate::ui::consts::MARGIN_Y),
                            row.y
                                - if matches!(self.route, StashArg::Performer(_)) {
                                    crate::ui::hero_content::pinned_name_bottom(cx.measure)
                                } else {
                                    self.reveal_top()
                                }
                                - card_row::heading_lift_max(&row.style),
                            max,
                        );
                    }
                }
                self.media(cx, fx);
                self.prefetch(Some(to.elem), fx);
                self.prefetch_shelf(to.elem, fx);
                if self
                    .position(to.elem)
                    .is_some_and(|(r, _)| r + 2 >= self.rows.len())
                {
                    self.next_shelf(fx);
                }
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind: InputKind::Text(edit),
                ..
            }) if self.state.editing != 0 => {
                let value = &mut self.state.query;
                let mut buffer =
                    crate::ui::text_buffer::TextBuffer::new(value.clone(), self.state.caret);
                buffer.edit(edit);
                self.state.caret = buffer.caret();
                *value = buffer.into_text();
                self.state.generation = self.state.generation.wrapping_add(1);
                self.state.loading = false;
                self.state.search_dirty_at = Some(cx.tick.ms);
                self.cancel_refresh();
                self.rebuild();
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Left | Key::Right,
                        edge: Edge::Down | Edge::Repeat,
                        ..
                    },
                ..
            }) if self.state.editing != 0 => {
                let edit = if matches!(
                    ev,
                    ScreenEvent::Input(InputEvent {
                        kind: InputKind::Key { key: Key::Left, .. },
                        ..
                    })
                ) {
                    TextEdit::Left
                } else {
                    TextEdit::Right
                };
                let value = &self.state.query;
                let mut buffer =
                    crate::ui::text_buffer::TextBuffer::new(value.clone(), self.state.caret);
                buffer.edit(&edit);
                self.state.caret = buffer.caret();
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Ok | Key::Back,
                        edge: Edge::Down,
                        ..
                    },
                ..
            }) if self.state.editing != 0 => {
                let search = self.state.editing == 1;
                self.state.editing = 0;
                fx.push(Fx::App(StashFx::Keyboard(false)));
                if search {
                    self.state.search_dirty_at = None;
                    self.state.page = 1;
                    self.state.append = false;
                    self.load(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Tick(t) => {
                if self
                    .state
                    .search_dirty_at
                    .is_some_and(|at| t.ms.wrapping_sub(at) >= 300)
                {
                    self.state.search_dirty_at = None;
                    self.state.page = 1;
                    self.state.append = false;
                    self.load(fx);
                }
                let focused_row = cx
                    .focus
                    .current
                    .and_then(|k| self.position(k.elem))
                    .and_then(|(r, _)| self.rows[r].collection.then_some(r));
                self.bands.focus(focused_row, true);
                self.bands.tick(crate::ui::consts::K_SCROLL, t.dt());
                if !self.home_shelves {
                    self.scroll_motion.step(
                        self.scroll_target,
                        crate::ui::consts::K_SCROLL,
                        t.dt(),
                    );
                    self.state.scroll = self.scroll_motion.pos;
                }
                let focused = cx.focus.current.and_then(|k| self.position(k.elem));
                for (i, row) in self.rows.iter_mut().enumerate() {
                    row.motion.update(
                        row.tiles.len(),
                        focused.filter(|(r, _)| *r == i).map(|(_, c)| c),
                        &row.style,
                        t.dt(),
                    );
                }
                if t.ms.wrapping_sub(self.state.media_at) >= 100 {
                    self.media(cx, fx);
                    self.state.media_at = t.ms;
                    self.prefetch(cx.focus.current.map(|k| k.elem), fx);
                }
                Handled::Yes
            }
            ScreenEvent::WillLeave(_) | ScreenEvent::Cover | ScreenEvent::Unmount => {
                self.state.covered = true;
                if self.state.editing != 0 {
                    self.state.editing = 0;
                    fx.push(Fx::App(StashFx::Keyboard(false)));
                }
                fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                Handled::Yes
            }
            ScreenEvent::StoreChanged(STASH_ACTIVITY, _)
                if !self.state.covered && !self.state.loading =>
            {
                self.refresh(fx);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
fn append_page(page: &mut PageData, data: &PageData) {
    let mut added = 0;
    for section in &data.sections {
        if let Some(existing) = page.sections.iter_mut().find(|s| {
            if section.shelf_id.is_some() {
                s.shelf_id == section.shelf_id
            } else {
                s.shelf_id.is_none() && s.title == section.title && s.shelf == section.shelf
            }
        }) {
            for tile in &section.tiles {
                if !existing.tiles.iter().any(|t| t.identity == tile.identity) {
                    existing.tiles.push(tile.clone());
                    added += 1;
                }
            }
        } else {
            added += section.tiles.len();
            page.sections.push(section.clone());
        }
    }
    for image in &data.images {
        if !page.images.iter().any(|i| i.id == image.id) {
            page.images.push(image.clone());
        }
    }
    page.count = data.count;
    page.has_more = data.has_more && added > 0;
    for scene in &data.scenes {
        if !page.scenes.iter().any(|existing| existing.id == scene.id) {
            page.scenes.push(scene.clone());
        }
    }
}
fn control_section(section: &Section) -> bool {
    section
        .tiles
        .first()
        .is_some_and(|t| t.identity.starts_with("control:"))
}
fn label(p: Painter, text: &str, rect: Rect, size: i32, color: [f32; 4]) {
    let text = std::ffi::CString::new(text.replace('\0', "")).unwrap();
    Label::new(text.as_ptr(), size, color).draw(p, rect);
}
impl Screen<StashHost> for StashScreen {
    fn links(&self, out: &mut Vec<Link>) {
        if !self.home_shelves && self.detail_inset.is_none() {
            out.push(Link {
                from: crate::ui::containers::tabs::STRIP,
                dir: Dir::Down,
                to: GroupId(0),
            });
            out.push(Link {
                from: GroupId(0),
                dir: Dir::Up,
                to: crate::ui::containers::tabs::STRIP,
            });
        }
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn name(&self) -> &'static str {
        match self.route {
            StashArg::Home => "home",
            StashArg::Scenes => "scenes",
            StashArg::Performers => "performers",
            StashArg::Galleries => "galleries",
            StashArg::Tags => "tags",
            StashArg::Search => "search",
            StashArg::Settings => "settings",
            StashArg::Scene(_) => "scene",
            StashArg::Performer(_) => "performer",
            StashArg::Tag(_) => "tag",
            StashArg::Gallery(_) => "gallery",
            StashArg::Viewer { .. } => "viewer",
            StashArg::Player(_) => "player",
        }
    }
    fn state(&self) -> &dyn LogicalState {
        &self.state
    }
    fn crumb(&self, _: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.route.label()))
    }
    fn prepare(&mut self, _: &mut Budget, _: &Cx<'_, StashHost>) {}
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let p = f.painter;
        for row in self
            .rows
            .iter()
            .filter(|r| !Self::pinned(r))
            .chain(self.rows.iter().filter(|r| Self::pinned(r)))
        {
            let y = self.row_y(row);
            if y > 1080.
                || y + row.style.h
                    < if Self::pinned(row) {
                        100.
                    } else {
                        self.content_view().y
                    }
            {
                continue;
            }
            let _clip = if Self::pinned(row) {
                None
            } else {
                Some(f.clip(p, self.content_view()))
            };
            let p = if Self::pinned(row) {
                p
            } else {
                p.clipped(self.content_view())
            };
            if !row.title.is_empty() {
                card_row::draw_heading(
                    p,
                    &row.title,
                    "",
                    MARGIN_X,
                    y - CARD_DY - TITLE_DY - row.motion.lift(),
                    SCR_W - 2. * MARGIN_X,
                    f.cx.measure,
                );
            }
            let buttons = row
                .tiles
                .first()
                .is_some_and(|tile| tile.identity.starts_with("control:"));
            if !buttons && !row.collection {
                let focused =
                    f.cx.focus
                        .current
                        .filter(|k| k.entry == self.entry)
                        .and_then(|k| row.keys.iter().position(|key| *key == k.elem));
                card_row::strip(
                    p,
                    &row.motion,
                    row.tiles.len(),
                    focused.map_or(-1, |i| i as i32),
                    y,
                    (row.style.w, row.style.h),
                    row.style.w + row.style.gap,
                    &row.style,
                    SCR_W,
                    |i| {
                        let tile = &row.tiles[i];
                        let preview = format!("preview:{}", tile.identity);
                        let image = if focused == Some(i) {
                            f.cx.views
                                .textures
                                .get(&preview)
                                .or_else(|| f.cx.views.textures.get(&tile.identity))
                        } else {
                            f.cx.views.textures.get(&tile.identity)
                        };
                        if let Some(metadata) = &tile.scene_metadata {
                            Art::Scene {
                                key: &tile.identity,
                                image: image.copied(),
                                background: f
                                    .cx
                                    .views
                                    .textures
                                    .get(&format!("blur:{}", tile.identity))
                                    .or_else(|| f.cx.views.textures.get(&tile.identity))
                                    .copied(),
                                aspect_ratio: metadata.aspect_ratio,
                            }
                        } else {
                            Art::Texture {
                                key: &tile.identity,
                                image: image.copied(),
                                portrait: tile.identity.starts_with("performer:"),
                            }
                        }
                    },
                    |_| None,
                    |i| {
                        let tile = &row.tiles[i];
                        let caption = tile
                            .scene_metadata
                            .as_ref()
                            .map(|m| {
                                crate::ui::widgets::scene_caption(
                                    &tile.caption,
                                    m.duration_seconds,
                                    m.rating100,
                                )
                            })
                            .unwrap_or_else(|| tile.caption.clone());
                        TileLabel::titled(&tile.title, &caption)
                    },
                    |p, i, x, focused| {
                        if let Some(count) = row.tiles[i].o_count {
                            let scale = row.motion.scale(i)
                                * if focused {
                                    crate::ui::press::scale()
                                } else {
                                    1.
                                };
                            crate::ui::widgets::o_count_mark(
                                p,
                                Rect::new(x, y, row.style.w, row.style.h).scaled(scale),
                                count,
                                f.cx.measure,
                            );
                        }
                    },
                    f.cx.measure,
                );
            }
            let focused_col =
                f.cx.focus
                    .current
                    .filter(|key| key.entry == self.entry)
                    .and_then(|key| row.keys.iter().position(|elem| *elem == key.elem));
            let order = (0..row.tiles.len())
                .filter(|index| Some(*index) != focused_col)
                .chain(focused_col);
            for c in order {
                let tile = &row.tiles[c];
                let key = row.keys[c];
                let Some(placed) = self.place(&key, f.cx, At::Drawn) else {
                    continue;
                };
                let rect = placed.rect;
                if rect.x + rect.w < 0. || rect.x > 1920. {
                    continue;
                }
                let focus =
                    f.cx.focus
                        .current
                        .is_some_and(|k| k.entry == self.entry && k.elem == key);
                f.stop(
                    p,
                    Stop {
                        key: FocusKey {
                            entry: self.entry,
                            elem: key,
                        },
                        rect,
                        rest_rect: placed.rest_rect,
                        clip: placed.clip,
                        hover: Hover::Focus,
                        activate: Activate::Press,
                    },
                );
                if row.collection {
                    let preview_key = format!("preview:{}", tile.identity);
                    let image = if focus {
                        f.cx.views
                            .textures
                            .get(&preview_key)
                            .or_else(|| f.cx.views.textures.get(&tile.identity))
                    } else {
                        f.cx.views.textures.get(&tile.identity)
                    };
                    let scale = row.motion.scale(c);
                    let lift =
                        ((scale - 1.) / (row.style.focus_scale - 1.).max(0.001)).clamp(0., 1.);
                    if row.style.w > row.style.h {
                        crate::ui::widgets::scene_card(
                            p,
                            rect,
                            image.copied(),
                            f.cx.views
                                .textures
                                .get(&format!("blur:{}", tile.identity))
                                .or_else(|| f.cx.views.textures.get(&tile.identity))
                                .copied(),
                            tile.scene_metadata.as_ref().and_then(|m| m.aspect_ratio),
                            row.style.tile_radius(rect, scale),
                            lift,
                        );
                    } else {
                        card_row::draw_tile(
                            p,
                            Art::Texture {
                                key: &tile.identity,
                                image: image.copied(),
                                portrait: tile.identity.starts_with("performer:"),
                            },
                            rect,
                            scale,
                            &row.style,
                            None,
                        );
                    }
                    if let Some(count) = tile.o_count {
                        crate::ui::widgets::o_count_mark(p, rect, count, f.cx.measure);
                    }
                    if matches!(self.route, StashArg::Tags) && image.is_none() {
                        let name = crate::text::elide_by(
                            &tile.title,
                            rect.w - 2. * theme::space::MD,
                            false,
                            |text| f.cx.measure.width_str(text, theme::size::CAPTION, true),
                        );
                        let name = std::ffi::CString::new(name.replace('\0', "")).unwrap();
                        Label::new(name.as_ptr(), theme::size::CAPTION, theme::TEXT_PRIMARY)
                            .bold()
                            .h(crate::ui::label::HAlign::Center)
                            .draw(p, rect);
                    }
                    if focus {
                        let expansion = self
                            .bands
                            .geometry()
                            .iter()
                            .find(|band| band.row == row.ordinal)
                            .map_or(0., |band| band.expansion);
                        let caption = tile
                            .scene_metadata
                            .as_ref()
                            .map(|m| {
                                crate::ui::widgets::scene_caption(
                                    &tile.caption,
                                    m.duration_seconds,
                                    m.rating100,
                                )
                            })
                            .unwrap_or_else(|| tile.caption.clone());
                        let title = TileLabel::titled(&tile.title, &caption)
                            .revealed(card_row::band_reveal(expansion));
                        let base_bottom = rect.cy() + rect.h / scale * 0.5;
                        card_row::draw_label_block(
                            p,
                            rect,
                            &row.style,
                            &title,
                            base_bottom + card_row::UNDER_DROP,
                            f.cx.measure,
                        );
                    }
                } else if tile.identity == "control:search" {
                    crate::ui::search_field::draw(
                        p,
                        rect,
                        &self.state.query,
                        self.state.caret,
                        self.state.editing != 0,
                        f.cx.tick.ms % 1000 < 500,
                        f32::from(focus),
                        f.cx.measure,
                    );
                } else if tile.identity.starts_with("control:") {
                    let label = if matches!(self.route, StashArg::Tags) && !tile.caption.is_empty()
                    {
                        format!("{} {}", tile.caption, tile.title)
                    } else {
                        tile.title.clone()
                    };
                    let title = crate::text::elide_by(&label, rect.w - 40., false, |s| {
                        let text = std::ffi::CString::new(s.replace('\0', "")).unwrap();
                        f.cx.measure.width(&text, theme::size::CAPTION, false)
                    });
                    let text = std::ffi::CString::new(title.replace('\0', "")).unwrap();
                    let button =
                        crate::ui::widgets::Button::new(text.as_ptr(), theme::size::CAPTION, rect)
                            .focused(focus);
                    button.draw(&crate::ui::Env::inert(), p);
                }
            }
        }
        let shelf_error = self
            .shelf_progress
            .values()
            .find(|p| !p.error.is_empty())
            .map(|p| p.error.as_str());
        if !self.state.error.is_empty() || shelf_error.is_some() {
            label(
                p,
                if self.state.error.is_empty() {
                    shelf_error.unwrap_or("")
                } else {
                    &self.state.error
                },
                Rect::new(72., 1000., 1776., 50.),
                theme::size::CAPTION,
                theme::TEXT_SECONDARY,
            );
        } else if self.state.loading {
            label(
                p,
                "Loading…",
                Rect::new(72., 1000., 1776., 50.),
                theme::size::CAPTION,
                theme::TEXT_SECONDARY,
            );
        } else if self.data.sections.iter().all(|s| s.tiles.is_empty())
            && (!matches!(self.route, StashArg::Search) || !self.state.query.trim().is_empty())
        {
            label(
                p,
                "No results",
                Rect::new(72., 400., 1776., 50.),
                theme::size::BODY,
                theme::TEXT_SECONDARY,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tile(id: &str) -> Tile {
        Tile {
            scene_metadata: None,
            identity: format!("scene:{id}"),
            title: id.into(),
            o_count: None,
            caption: String::new(),
            image: None,
            preview: None,
            action: Action::Open(StashArg::Scene(id.into())),
        }
    }
    fn data(ids: &[&str]) -> PageData {
        PageData {
            title: "Scenes".into(),
            sections: vec![Section {
                shelf_id: None,
                title: "Scenes".into(),
                tiles: ids.iter().map(|id| tile(id)).collect(),
                portrait: false,
                shelf: false,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn collection_viewport_includes_pixels_above_controls() {
        let screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        assert_eq!(screen.content_view().y, 0.);
        assert_eq!(screen.content_view().h, SCR_H);
        assert!(screen.reveal_top() > crate::ui::widgets::CHIP_FRAME.y);
    }
    #[test]
    fn pinned_name_blocks_hidden_pointer_targets_without_clipping_collection_pixels() {
        let mut screen = StashScreen::new(StashArg::Performer("1".into()), EntryId(1));
        screen.hit_clearance(271.);
        assert_eq!(screen.content_view().y, 0.);
        assert_eq!(screen.content_view().h, SCR_H);
        assert_eq!(screen.hit_view().y, 271.);
        screen.hit_clearance(0.);
        assert_eq!(screen.hit_view().y, 0.);
    }
    #[test]
    fn refresh_keeps_keys_when_order_changes() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = data(&["a", "b"]);
        screen.rebuild();
        let key = screen.keys["Scenes:scene:b"];
        screen.data = data(&["b", "a", "c"]);
        screen.rebuild();
        assert_eq!(screen.keys["Scenes:scene:b"], key);
        let (r, c) = screen.position(key).unwrap();
        assert_eq!(screen.rows[r].tiles[c].identity, "scene:b");
    }
    #[test]
    fn same_numeric_provider_ids_do_not_share_focus() {
        let mut screen = StashScreen::new(StashArg::Home, EntryId(1));
        screen.data = data(&["1"]);
        let mut person = tile("1");
        person.identity = "performer:1".into();
        screen.data.sections.push(Section {
            shelf_id: None,
            title: "Performers".into(),
            tiles: vec![person],
            portrait: true,
            shelf: false,
        });
        screen.rebuild();
        assert_ne!(
            screen.keys["Scenes:scene:1"],
            screen.keys["Performers:performer:1"]
        );
    }
    #[test]
    fn scene_appearing_in_carousel_and_tag_has_distinct_seats() {
        let mut screen = StashScreen::new(StashArg::Home, EntryId(1));
        screen.data = data(&["a"]);
        screen.data.sections.push(Section {
            shelf_id: None,
            title: "Tag 1".into(),
            tiles: vec![tile("a")],
            portrait: false,
            shelf: false,
        });
        screen.rebuild();
        assert_ne!(screen.rows[0].keys[0], screen.rows[1].keys[0]);
    }
    #[test]
    fn initial_query_is_first_page_and_performer_sort_descending() {
        let screen = StashScreen::new(StashArg::Performers, EntryId(1));
        let query = screen.query();
        assert_eq!(query.page, 1);
        assert_eq!(query.per_page, 50);
        assert_eq!(query.sort, "o_counter");
        assert_eq!(query.direction, Direction::Descending);
    }
    #[test]
    fn paginated_catalog_has_no_manual_next_page_control() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = data(&["a"]);
        screen.data.count = 100;
        screen.rebuild();
        assert!(!screen
            .rows
            .iter()
            .flat_map(|r| &r.tiles)
            .any(|t| t.identity == "control:more"));
    }
    #[test]
    fn home_catalog_only_projects_shelves_below_the_billboard() {
        let mut screen = StashScreen::home(EntryId(1));
        screen.data = data(&["a"]);
        screen.data.sections.push(Section {
            shelf_id: None,
            title: "Performers".into(),
            tiles: vec![tile("b")],
            portrait: true,
            shelf: false,
        });
        screen.rebuild();
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(screen.rows[0].title, "Performers");
        assert_eq!(screen.rows[0].y, PEEK_Y + CARD_DY);
        assert_eq!(
            screen.home_scroll_target(screen.rows[0].keys[0]),
            Some(PEEK_Y - GRID_TOP_Y)
        );
        assert_eq!(screen.first_group(), Some(GroupId(1)));
    }
    #[test]
    fn browsing_scroll_keeps_controls_pinned_and_omits_duplicate_heading() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = data(&["a", "b", "c", "d", "e"]);
        screen.rebuild();
        let nav_y = screen.row_y(&screen.rows[0]);
        let content_y = screen.row_y(&screen.rows[1]);
        screen.state.scroll = 250.;
        assert_eq!(screen.row_y(&screen.rows[0]), nav_y);
        assert_eq!(screen.row_y(&screen.rows[1]), content_y - 250.);
        assert!(screen.rows[1].title.is_empty());
        screen.grid(|grid| assert_eq!(grid.shelves[0].group, GroupId(0)));
    }
    #[test]
    fn catalog_grid_clears_shared_top_chrome_without_redundant_controls() {
        let mut screen = StashScreen::new(StashArg::Performers, EntryId(1));
        screen.data = data(&["a"]);
        screen.data.sections[0].portrait = true;
        screen.rebuild();
        assert_eq!(screen.rows.len(), 1);
        assert!(screen.rows[0].collection);
        let chip = crate::ui::widgets::CHIP_FRAME;
        assert!(screen.rows[0].y > chip.y + chip.h + theme::space::MD);
        assert!(screen.rows[0].y >= screen.content_view().y);
        assert!(screen.rows[0].title.is_empty());
    }
    #[test]
    fn detail_error_controls_keep_document_flow_and_scroll_with_content() {
        let mut screen = StashScreen::new(StashArg::Performer("1".into()), EntryId(1));
        screen.data = data(&["a"]);
        screen.data.performer = Some(crate::stash::Performer::default());
        screen.detail_inset = Some(800.);
        screen.state.error = "Disconnected".into();
        screen.rebuild();
        assert!(!StashScreen::pinned(&screen.rows[0]));
        assert!(screen.rows[1].y > screen.rows[0].y + screen.rows[0].style.h);
        let y = screen.row_y(&screen.rows[0]);
        screen.state.scroll = 100.;
        assert_eq!(screen.row_y(&screen.rows[0]), y - 100.);
    }
    #[test]
    fn tags_use_square_collection_cells_without_refresh_or_search() {
        let mut screen = StashScreen::new(StashArg::Tags, EntryId(1));
        screen.data = data(&["a", "b"]);
        screen.rebuild();
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(screen.rows[0].tiles.len(), 2);
        assert_eq!(screen.rows[0].style.w, screen.rows[0].style.h);
        assert!(screen.rows[0].collection);
        assert!(!screen
            .rows
            .iter()
            .flat_map(|row| &row.tiles)
            .any(|tile| tile.identity.starts_with("control:")));
    }
    #[test]
    fn scene_grid_uses_four_uniform_landscape_cells_and_short_final_row() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = data(&["a", "b", "c", "d", "e"]);
        screen.rebuild();
        assert_eq!(screen.rows[1].tiles.len(), 4);
        assert_eq!(screen.rows[2].tiles.len(), 1);
        assert!(screen.rows[1].collection);
        assert!(screen.rows[1].style.w > screen.rows[1].style.h);
        assert_eq!(screen.rows[0].tiles.len(), 1);
        assert_eq!(screen.rows[0].tiles[0].identity, "control:sort");
    }
    #[test]
    fn performer_favorite_sections_remain_shelves_above_the_scene_grid() {
        let mut screen = StashScreen::new(StashArg::Performer("1".into()), EntryId(1));
        screen.data = data(&["a"]);
        screen.data.sections.insert(
            0,
            Section {
                shelf_id: None,
                title: "Favorite tag".into(),
                tiles: vec![tile("b")],
                portrait: false,
                shelf: true,
            },
        );
        screen.rebuild();
        assert!(!screen.rows[0].collection);
        assert!(screen.rows[1].collection);
    }
    #[test]
    fn full_portrait_scene_is_contained_in_landscape_frame() {
        let frame = Rect::new(0., 0., 420., 236.);
        let portrait = crate::ui::widgets::contain_frame(frame, 540., 960.);
        assert_eq!(portrait.h, frame.h);
        assert!(portrait.w < frame.w);
        assert_eq!(portrait.cx(), frame.cx());
    }
    fn fifty_scenes() -> PageData {
        let ids = (1..=50).map(|id| id.to_string()).collect::<Vec<_>>();
        let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
        let mut page = data(&refs);
        page.count = 120;
        page.has_more = true;
        page
    }
    #[test]
    fn prefetch_requests_once_near_end_and_never_while_covered() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = fifty_scenes();
        screen.rebuild();
        let start = screen.test_content_key("scene:1").unwrap();
        let end = screen.test_content_key("scene:45").unwrap();
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.prefetch(Some(start), &mut fx);
            assert!(!screen.state.loading);
            screen.state.covered = true;
            screen.prefetch(Some(end), &mut fx);
            assert!(!screen.state.loading);
            screen.state.covered = false;
            screen.prefetch(Some(end), &mut fx);
            screen.prefetch(Some(end), &mut fx);
        }
        assert_eq!(screen.state.page, 2);
        assert_eq!(
            out.iter()
                .filter(|f| matches!(&f.fx, Fx::App(StashFx::Work(_, Work::Load { .. }))))
                .count(),
            1
        );
        assert!(out.iter().any(|f| matches!(&f.fx, Fx::App(StashFx::Work(_, Work::Load { query, .. })) if query.page == 2 && query.per_page == 50)));
    }
    #[test]
    fn overlapping_append_retains_focus_and_duplicate_page_ends_prefetch() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = fifty_scenes();
        screen.rebuild();
        let key = screen.test_content_key("scene:45").unwrap();
        screen.state.append = true;
        let mut next = data(&["50", "51", "52"]);
        next.count = 120;
        next.has_more = true;
        screen.land_page(&next);
        screen.rebuild();
        assert_eq!(screen.data.sections[0].tiles.len(), 52);
        assert_eq!(screen.test_content_key("scene:45"), Some(key));
        assert!(screen.data.has_more);
        screen.land_page(&next);
        assert!(!screen.data.has_more);
        assert_eq!(screen.data.sections[0].tiles.len(), 52);
    }
    #[test]
    fn home_keeps_more_than_one_page_in_a_single_original_strip() {
        let mut screen = StashScreen::home(EntryId(1));
        screen.data = fifty_scenes();
        let mut people = fifty_scenes().sections.remove(0);
        people.title = "Performers".into();
        people.portrait = true;
        screen.data.sections.push(people);
        screen.rebuild();
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(screen.rows[0].tiles.len(), 50);
    }
    #[test]
    fn home_first_column_up_targets_immediately_previous_shelf() {
        let config = Config::default();
        let textures = HashMap::new();
        let playback = PlaybackView::default();
        let cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: &playback,
            },
            tick: Tick::default(),
            measure: &Measure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut screen = StashScreen::home(EntryId(1));
        screen.data = data(&["hero"]);
        for name in ["Performers", "Tag A", "Tag B"] {
            let mut section = data(&["a", "b", "c"]).sections.remove(0);
            section.title = name.into();
            screen.data.sections.push(section);
        }
        screen.rebuild();
        screen.state.scroll = screen.home_scroll_target(screen.rows[2].keys[0]).unwrap();
        for col in [0, 2] {
            let from = FocusKey {
                entry: screen.entry,
                elem: screen.rows[2].keys[col],
            };
            assert!(
                matches!(screen.neighbour(from,Dir::Up,&cx),Step::Move(k) if k.elem==screen.rows[1].keys[col])
            );
        }
        assert!(matches!(
            screen.neighbour(
                FocusKey {
                    entry: screen.entry,
                    elem: screen.rows[0].keys[0]
                },
                Dir::Up,
                &cx
            ),
            Step::Edge
        ));
        screen.data.sections[1].portrait = true;
        screen.rebuild();
        let from = FocusKey {
            entry: screen.entry,
            elem: screen.rows[1].keys[1],
        };
        let source = screen.place(&from.elem, &cx, At::SpringTarget).unwrap().rect;
        let Step::Move(to) = screen.neighbour(from, Dir::Up, &cx) else {
            panic!("adjacent shelf missing")
        };
        let dest = screen.place(&to.elem, &cx, At::SpringTarget).unwrap().rect;
        assert!(
            (source.cx() - dest.cx()).abs()
                <= (screen.rows[0].style.w + screen.rows[0].style.gap) * 0.5
        );
    }
    fn shelf_screen() -> (StashScreen, ShelfSpec) {
        let mut screen = StashScreen::new(StashArg::Performer("1".into()), EntryId(1));
        let spec = ShelfSpec {
            id: ShelfId::Favorites,
            title: "Favorites".into(),
            scope: ShelfScope::Performer("1".into()),
        };
        screen.data = data(&["a"]);
        screen.data.shelves = vec![spec.clone()];
        screen.shelf_generation = 7;
        screen.shelf_progress.insert(
            spec.id.clone(),
            ShelfProgress {
                loading: true,
                ..Default::default()
            },
        );
        (screen, spec)
    }
    #[test]
    fn fresh_tag_landing_follows_first_shelf_but_restored_focus_keeps_its_grid() {
        let config = Config::default();
        let textures = HashMap::new();
        let playback = PlaybackView::default();
        for restored in [false, true] {
            let mut screen = StashScreen::new(StashArg::Tag("1".into()), EntryId(1));
            screen.data = data(&["a"]);
            screen.rebuild();
            let old = screen.rows[0].keys[0];
            let cx = Cx {
                views: Views {
                    textures: &textures,
                    config: &config,
                    playback: &playback,
                },
                tick: Tick::default(),
                measure: &Measure,
                press: PressRead::default(),
                focus: FocusRead {
                    current: Some(FocusKey {
                        entry: screen.entry,
                        elem: old,
                    }),
                    ..Default::default()
                },
                owner: InputOwner::Entry(screen.entry),
            };
            let spec = ShelfSpec {
                id: ShelfId::Favorites,
                title: "Favorites".into(),
                scope: ShelfScope::Tag("1".into()),
            };
            screen.data.shelves = vec![spec.clone()];
            screen.shelf_generation = 7;
            screen.shelf_progress.insert(
                spec.id.clone(),
                ShelfProgress {
                    loading: true,
                    ..Default::default()
                },
            );
            let mut out = Vec::new();
            let mut present = crate::ui::present::Present::new();
            {
                let mut fx =
                    Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                if restored {
                    screen.step(&ScreenEvent::Enter(Enter::Restored), &cx, &mut fx);
                }
                screen.step(
                    &ScreenEvent::Async(
                        RequestId(7),
                        StashMsg::ShelfPageLoaded {
                            scope: spec.scope.clone(),
                            id: spec.id.clone(),
                            generation: 7,
                            page: 1,
                            result: Ok(crate::stores::stash::fixture_shelf(&spec, 1)),
                        },
                    ),
                    &cx,
                    &mut fx,
                );
            }
            let seats = out
                .iter()
                .filter(|e| {
                    matches!(
                        &e.fx,
                        Fx::Deliver(_, Delivery::Screen(ScreenEvent::Enter(Enter::Fresh { .. })))
                    )
                })
                .count();
            assert_eq!(seats, usize::from(!restored));
            assert_eq!(screen.test_focus_identity(old), Some("scene:a"));
            if !restored {
                assert_eq!(screen.state.scroll, 0.);
                assert_eq!(screen.rows[0].shelf_id, Some(ShelfId::Favorites));
            }
        }
    }
    #[test]
    fn shelf_pages_append_past_fifty_preserve_keys_and_reject_stale_scope() {
        let (mut screen, spec) = shelf_screen();
        let first = crate::stores::stash::fixture_shelf(&spec, 1);
        assert!(!screen.land_shelf(
            &ShelfScope::Performer("2".into()),
            &spec.id,
            7,
            1,
            &Ok(first.clone())
        ));
        assert!(!screen.land_shelf(&spec.scope, &spec.id, 6, 1, &Ok(first.clone())));
        assert!(screen.land_shelf(&spec.scope, &spec.id, 7, 1, &Ok(first)));
        let key = screen.rows[0].keys[0];
        let identity = screen.rows[0].tiles[0].identity.clone();
        let mut second = crate::stores::stash::fixture_shelf(&spec, 2);
        // Independently exercise appending even a shelf whose synthetic favorites are sparse.
        second.section.tiles = (51..=100).map(|n| tile(&n.to_string())).collect();
        second.has_more = true;
        screen.shelf_progress.get_mut(&spec.id).unwrap().loading = true;
        assert!(!screen.land_shelf(&spec.scope, &spec.id, 7, 3, &Ok(second.clone())));
        assert!(screen.land_shelf(&spec.scope, &spec.id, 7, 2, &Ok(second)));
        assert_eq!(screen.test_focus_identity(key), Some(identity.as_str()));
        assert!(screen.rows[0].tiles.len() > 50);
        assert!(!screen.land_shelf(
            &spec.scope,
            &spec.id,
            7,
            2,
            &Ok(crate::stores::stash::fixture_shelf(&spec, 2))
        ));
    }
    #[test]
    fn shelf_prefetch_single_inflight_and_failure_retries_same_page() {
        let (mut screen, spec) = shelf_screen();
        let mut response = crate::stores::stash::fixture_shelf(&spec, 1);
        response.section.tiles = (1..=50).map(|n| tile(&n.to_string())).collect();
        response.has_more = true;
        response.count = 120;
        screen.land_shelf(&spec.scope, &spec.id, 7, 1, &Ok(response));
        let near = screen.rows[0].keys[38];
        let early = screen.rows[0].keys[0];
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.prefetch_shelf(early, &mut fx);
            assert!(out.is_empty());
        }
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.prefetch_shelf(near, &mut fx);
            screen.prefetch_shelf(near, &mut fx);
        }
        assert_eq!(out.len(), 1);
        screen.land_shelf(&spec.scope, &spec.id, 7, 2, &Err("Disconnected".into()));
        assert_eq!(screen.shelf_progress[&spec.id].page, 1);
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.request_shelf(spec.clone(), &mut fx);
        }
        assert_eq!(out.len(), 2);
        assert!(matches!(
            &out[1].fx,
            Fx::App(StashFx::Work(_, Work::ShelfPage { page: 2, .. }))
        ));
    }
    #[test]
    fn shelf_identity_survives_label_changes_and_duplicate_labels() {
        let (mut screen, spec) = shelf_screen();
        let mut first = crate::stores::stash::fixture_shelf(&spec, 1);
        first.section.tiles = vec![tile("1")];
        screen.land_shelf(&spec.scope, &spec.id, 7, 1, &Ok(first));
        let key = screen.rows[0].keys[0];
        screen.data.sections[0].title = "Localized name".into();
        screen.rebuild();
        assert_eq!(screen.rows[0].keys[0], key);
        let mut other = screen.data.sections[0].clone();
        other.shelf_id = Some(ShelfId::Tag("different".into()));
        screen.data.sections.insert(1, other);
        screen.rebuild();
        assert_ne!(screen.rows[0].keys[0], screen.rows[1].keys[0]);
    }
    struct Measure;
    impl crate::ui::machine::Measure for Measure {
        fn width(&self, _: &std::ffi::CStr, _: i32, _: bool) -> f32 {
            10.
        }
        fn cap_h(&self, _: i32) -> f32 {
            10.
        }
        fn line_h(&self, _: i32) -> f32 {
            10.
        }
    }
    #[test]
    fn refresh_stages_all_loaded_pages_before_replacing_focused_tail() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = fifty_scenes();
        screen.state.append = true;
        screen.land_page(&data(&["51", "52"]));
        screen.rebuild();
        screen.state.loaded_pages = 2;
        let key = screen.test_content_key("scene:51").unwrap();
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        screen.refresh(&mut fx);
        let mut first = fifty_scenes();
        first.sections[0].tiles[0].title = "Refreshed".into();
        assert!(!screen.land_refresh(&first, &mut fx));
        assert_eq!(screen.data.sections[0].tiles[0].title, "1");
        assert_eq!(screen.test_content_key("scene:51"), Some(key));
        assert!(screen.land_refresh(&data(&["51", "52"]), &mut fx));
        screen.rebuild();
        assert_eq!(screen.data.sections[0].tiles[0].title, "Refreshed");
        assert_eq!(screen.test_content_key("scene:51"), Some(key));
        assert_eq!(screen.state.page, 2);
        assert!(screen.refresh_data.is_none());
    }
    #[test]
    fn search_focuses_field_opens_keyboard_and_debounces_scene_requests() {
        let _lock = crate::testlock::serial();
        let config = Config::default();
        let textures = HashMap::new();
        let playback = PlaybackView::default();
        let cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: &playback,
            },
            tick: Tick::default(),
            measure: &Measure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut screen = StashScreen::new(StashArg::Search, EntryId(1));
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.step(&ScreenEvent::Mount, &cx, &mut fx);
        }
        assert_eq!(
            out.iter()
                .filter(|event| matches!(event.fx, Fx::App(StashFx::Keyboard(true))))
                .count(),
            1
        );
        assert!(out.iter().any(|event| matches!(&event.fx, Fx::Deliver(_, Delivery::Screen(ScreenEvent::Enter(Enter::Fresh { focus: FocusTarget::Elem(key) }))) if key.elem == screen.keys["control:search"])));
        out.clear();
        let old_generation = screen.state.generation;
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.step(
                &ScreenEvent::Input(InputEvent {
                    at: Tick::default(),
                    source: Source::Script,
                    kind: InputKind::Text(TextEdit::Commit("cats".into())),
                }),
                &cx,
                &mut fx,
            );
            screen.step(
                &ScreenEvent::Tick(Tick {
                    ms: 299,
                    dt_us: 16000,
                }),
                &cx,
                &mut fx,
            );
            screen.step(
                &ScreenEvent::Async(
                    RequestId(old_generation),
                    StashMsg::Loaded {
                        generation: old_generation,
                        result: Ok(data(&["stale"])),
                    },
                ),
                &cx,
                &mut fx,
            );
        }
        assert!(!out
            .iter()
            .any(|event| matches!(event.fx, Fx::App(StashFx::Work(_, Work::Load { .. })))));
        assert!(screen.data.sections.is_empty());
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.step(
                &ScreenEvent::Tick(Tick {
                    ms: 300,
                    dt_us: 16000,
                }),
                &cx,
                &mut fx,
            );
            screen.step(
                &ScreenEvent::Tick(Tick {
                    ms: 600,
                    dt_us: 16000,
                }),
                &cx,
                &mut fx,
            );
        }
        assert_eq!(out.iter().filter(|event| matches!(&event.fx, Fx::App(StashFx::Work(_, Work::Load { route: StashArg::Search, query, .. })) if query.q == "cats")).count(), 1);
    }
    #[test]
    fn stale_network_generation_cannot_replace_visible_results() {
        let _lock = crate::testlock::serial();
        let config = Config::default();
        let textures = HashMap::new();
        let cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: &PlaybackView::default(),
            },
            tick: Tick::default(),
            measure: &Measure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.state.generation = 3;
        screen.data = data(&["current"]);
        screen.rebuild();
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        let event = ScreenEvent::Async(
            RequestId(2),
            StashMsg::Loaded {
                generation: 2,
                result: Ok(data(&["stale"])),
            },
        );
        assert!(matches!(screen.step(&event, &cx, &mut fx), Handled::No));
        assert_eq!(screen.data.sections[0].tiles[0].identity, "scene:current");
    }
}
