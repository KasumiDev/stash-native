//! Stash screens compose shared CardRow geometry; Input owns every cursor.
use crate::screens::stash_registry::*;
#[cfg(test)]
use crate::stash::Config;
use crate::stash::{Direction, Query};
use crate::stores::stash::{PageData, Section, Tile, Work};
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
    loading: bool,
    error: String,
    query: String,
    page: u32,
    sort: usize,
    scroll: f32,
    editing: u8,
    caret: usize,
    shelf_loading: bool,
    append: bool,
    media_at: u32,
    covered: bool,
}
impl LogicalState for State {
    fn write(&self, c: &mut Canon) {
        c.u32(self.generation)
            .bool(self.loading)
            .str(&self.error)
            .str(&self.query)
            .u32(self.page)
            .u32(self.sort as u32)
            .f32(self.scroll)
            .u32(self.editing as u32)
            .u32(self.caret as u32)
            .bool(self.covered);
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!("page={} loading={}", self.page, self.loading));
    }
}
struct Row {
    title: String,
    tiles: Vec<Tile>,
    keys: Vec<u32>,
    motion: CardRow,
    style: RowStyle,
    y: f32,
}
const TITLE_Y: f32 = GRID_TOP_Y - TITLE_DY;
const TITLE_H: f32 = TITLE_DY + theme::space::MD;
const CONTROLS_Y: f32 = TITLE_Y + TITLE_H + theme::space::SM;
const CONTENT_TOP: f32 = CONTROLS_Y + theme::space::XL + theme::space::MD;
const CONTENT_VIEW: Rect = Rect::new(0., CONTENT_TOP, SCR_W, SCR_H - CONTENT_TOP);
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
    scroll_motion: crate::ui::Spring,
    scroll_target: f32,
}
impl StashScreen {
    fn pinned(row: &Row) -> bool {
        row.tiles
            .first()
            .is_some_and(|t| t.identity.starts_with("control:"))
    }
    fn row_y(&self, row: &Row) -> f32 {
        if Self::pinned(row) {
            row.y
        } else {
            row.y - self.state.scroll
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
        Self {
            route,
            entry,
            state: State {
                page: 1,
                ..Default::default()
            },
            data: PageData::default(),
            rows: Vec::new(),
            keys: HashMap::new(),
            next_key: 1,
            home_shelves: false,
            detail_inset: None,
            scroll_motion: crate::ui::Spring::at(0.),
            scroll_target: 0.,
        }
    }
    pub(crate) fn home(entry: EntryId) -> Self {
        let mut screen = Self::new(StashArg::Home, entry);
        screen.home_shelves = true;
        screen
    }
    pub(crate) fn scenes(&self) -> &[crate::stash::Scene] {
        &self.data.scenes
    }
    pub(crate) fn loading(&self) -> bool {
        self.state.loading
    }
    pub(crate) fn error(&self) -> &str {
        &self.state.error
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
    pub(crate) fn home_scroll_target(&self, key: u32) -> Option<f32> {
        self.position(key)
            .map(|(r, _)| (self.rows[r].y - GRID_TOP_Y - CARD_DY).max(PEEK_Y - GRID_TOP_Y))
    }
    fn content_view(&self) -> Rect {
        if self.home_shelves {
            let top = GRID_TOP_Y - TITLE_DY - theme::space::MD;
            Rect::new(0., top, SCR_W, SCR_H - top)
        } else {
            CONTENT_VIEW
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
        self.state.shelf_loading = false;
        self.state.error.clear();
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
            self.data = data.clone();
            return;
        }
        let mut added = 0;
        for section in &data.sections {
            if let Some(existing) = self
                .data
                .sections
                .iter_mut()
                .find(|s| s.title == section.title)
            {
                for tile in &section.tiles {
                    if !existing.tiles.iter().any(|t| t.identity == tile.identity) {
                        existing.tiles.push(tile.clone());
                        added += 1;
                    }
                }
            } else {
                added += section.tiles.len();
                self.data.sections.push(section.clone());
            }
        }
        for image in &data.images {
            if !self.data.images.iter().any(|i| i.id == image.id) {
                self.data.images.push(image.clone());
            }
        }
        self.data.count = data.count;
        self.data.has_more = data.has_more && added > 0;
    }
    fn near_page_end(&self, focused: Option<u32>) -> bool {
        self.data
            .sections
            .iter()
            .filter(|section| {
                !matches!(self.route, StashArg::Home) || section.title == "Performers"
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
            || self.state.shelf_loading
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
    fn next_shelf(&mut self, fx: &mut Effects<'_, StashHost>) {
        if self.state.covered {
            return;
        }
        if self.state.shelf_loading || self.data.lazy_tags.is_empty() {
            return;
        }
        self.state.shelf_loading = true;
        let tag = self.data.lazy_tags.remove(0);
        fx.push(Fx::App(StashFx::Work(
            Addr {
                to: fx.from(),
                req: RequestId(self.state.generation),
            },
            Work::TagShelf {
                tag,
                generation: self.state.generation,
            },
        )));
    }
    fn control(id: &str, title: String, action: Action) -> Tile {
        Tile {
            identity: format!("control:{id}"),
            title,
            caption: String::new(),
            image: None,
            preview: None,
            action,
        }
    }
    fn rebuild(&mut self) {
        let mut sections = Vec::new();
        let mut controls = vec![Self::control(
            "refresh",
            if self.state.loading {
                "Loading…"
            } else if !self.state.error.is_empty() {
                "Retry"
            } else {
                "Refresh"
            }
            .into(),
            Action::Refresh,
        )];
        if !self.home_shelves && self.detail_inset.is_none() {
            controls.push(Self::control(
                "search",
                format!("Search: {}", self.state.query),
                Action::EditSearch,
            ));
            if matches!(self.route, StashArg::Scenes | StashArg::Search) {
                controls.push(Self::control(
                    "sort",
                    format!(
                        "Sort: {}",
                        ["Date", "Title", "O count"][self.state.sort % 3]
                    ),
                    Action::Sort,
                ));
            }
        }
        if !self.home_shelves
            && (self.detail_inset.is_none()
                || self.data.scene.is_none()
                || !self.state.error.is_empty())
        {
            if self.detail_inset.is_some() {
                controls.truncate(1);
            }
            sections.push(Section {
                title: String::new(),
                tiles: controls,
                portrait: false,
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
        let mut old_motion = std::mem::take(&mut self.rows).into_iter();
        let mut y = if self.detail_inset.is_some() && self.data.scene.is_none() {
            CONTROLS_Y
        } else if let Some(inset) = self.detail_inset {
            inset
        } else if self.home_shelves {
            PEEK_Y
        } else {
            CONTROLS_Y
        };
        for (section_index, s) in sections.into_iter().enumerate() {
            let control = s
                .tiles
                .first()
                .is_some_and(|t| t.identity.starts_with("control:"));
            let style = if control {
                RowStyle {
                    w: if section_index == 0 { 225. } else { 405. },
                    h: 64.,
                    gap: 20.,
                    margin_x: MARGIN_X,
                    ..RowStyle::EPISODE
                }
            } else if matches!(self.route, StashArg::Tags) {
                RowStyle {
                    w: SCR_W - 2. * MARGIN_X,
                    h: 72.,
                    gap: 0.,
                    margin_x: MARGIN_X,
                    focus_scale: 1.02,
                    ..RowStyle::EPISODE
                }
            } else if s.portrait {
                RowStyle::HOME
            } else {
                RowStyle::EPISODE
            };
            let chunk = if matches!(self.route, StashArg::Home) || control {
                s.tiles.len().max(1)
            } else if matches!(self.route, StashArg::Tags) {
                1
            } else if s.portrait {
                crate::ui::poster_grid::COLS
            } else {
                4
            };
            for (i, tiles) in s.tiles.chunks(chunk).enumerate() {
                if !control {
                    y += if i == 0 {
                        if self.home_shelves || s.title == self.data.title {
                            CARD_DY
                        } else {
                            TITLE_DY + CARD_DY
                        }
                    } else if matches!(self.route, StashArg::Tags) {
                        theme::space::SM
                    } else {
                        theme::space::MD
                    };
                }
                let keys = tiles
                    .iter()
                    .map(|t| {
                        let identity = if control {
                            t.identity.clone()
                        } else {
                            format!("{}:{}", s.title, t.identity)
                        };
                        let key = self.keys.entry(identity).or_insert_with(|| {
                            let k = self.next_key;
                            self.next_key += 1;
                            k
                        });
                        *key
                    })
                    .collect();
                self.rows.push(Row {
                    title: if i == 0 && s.title != self.data.title {
                        s.title.clone()
                    } else {
                        String::new()
                    },
                    tiles: tiles.to_vec(),
                    keys,
                    motion: old_motion
                        .next()
                        .map(|r| r.motion)
                        .unwrap_or_else(CardRow::new),
                    style,
                    y,
                });
                y += style.h
                    + if control {
                        theme::space::MD
                    } else if matches!(self.route, StashArg::Tags) {
                        theme::space::SM
                    } else {
                        TileLabel::height(true)
                    };
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
            if self.row_y(row) > 1080. || self.row_y(row) + row.style.h < CONTENT_VIEW.y {
                continue;
            }
            if matches!(self.route, StashArg::Tags) {
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
                }
            }
        }
        let preview = cx
            .focus
            .current
            .and_then(|k| self.position(k.elem))
            .and_then(|(r, c)| self.rows[r].tiles.get(c))
            .and_then(|t| {
                t.preview
                    .as_ref()
                    .map(|url| (t.identity.clone(), url.clone()))
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
                if self.state.loading {
                    return;
                }
                if !self.state.append {
                    self.state.page = 1;
                }
                self.load(fx);
            }
            Action::Sort => {
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
                    place.clip = place.clip.intersect(self.content_view());
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
        match ev {
            ScreenEvent::Mount => {
                self.rebuild();
                self.load(fx);
                Handled::Yes
            }
            ScreenEvent::Uncover => {
                self.state.covered = false;
                if !self.state.loading {
                    self.state.page = 1;
                    self.state.append = false;
                    self.load(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Async(_, StashMsg::Loaded { generation, result })
                if *generation == self.state.generation && self.state.loading =>
            {
                self.state.loading = false;
                match result {
                    Ok(data) => {
                        self.land_page(data);
                        self.state.append = false;
                        self.rebuild();
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
            ScreenEvent::Async(_, StashMsg::ShelfLoaded { generation, result })
                if *generation == self.state.generation =>
            {
                self.state.shelf_loading = false;
                match result {
                    Ok(section) => {
                        if !section.tiles.is_empty() {
                            self.data.sections.push(section.clone());
                            self.rebuild();
                            self.media(cx, fx);
                        } else {
                            self.next_shelf(fx);
                        }
                    }
                    Err(e) => self.state.error = e.clone(),
                }
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
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
                if let Some((r, _)) = self.position(to.elem) {
                    let row = &self.rows[r];
                    if !Self::pinned(row) && !self.home_shelves {
                        let max = self
                            .rows
                            .last()
                            .map_or(0., |last| {
                                last.y + last.style.h + TileLabel::height(true) - SCR_H
                            })
                            .max(0.);
                        self.scroll_target = card_row::reveal(
                            self.state.scroll,
                            row.y + row.style.h + TileLabel::height(true) - SCR_H,
                            row.y - self.content_view().y - card_row::heading_lift_max(&row.style),
                            max,
                        );
                    }
                }
                self.media(cx, fx);
                self.prefetch(Some(to.elem), fx);
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
                    self.state.page = 1;
                    self.state.append = false;
                    self.load(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Tick(t) => {
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
                fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                Handled::Yes
            }
            ScreenEvent::StoreChanged(STASH_ACTIVITY, _)
                if !self.state.covered && !self.state.loading =>
            {
                self.state.page = 1;
                self.state.append = false;
                self.load(fx);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
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
        let title = if self.data.title.is_empty() {
            self.route.label()
        } else {
            &self.data.title
        };
        if !self.home_shelves && self.detail_inset.is_none() {
            label(
                p,
                title,
                Rect::new(MARGIN_X, TITLE_Y, SCR_W - 2. * MARGIN_X, TITLE_H),
                theme::size::TITLE,
                theme::TEXT_PRIMARY,
            );
        }
        for row in &self.rows {
            let y = self.row_y(row);
            if y > 1080.
                || y + row.style.h
                    < if Self::pinned(row) {
                        100.
                    } else {
                        CONTENT_VIEW.y
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
            let buttons = Self::pinned(row) || matches!(self.route, StashArg::Tags);
            if !buttons {
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
                        Art::Texture {
                            key: &tile.identity,
                            image: image.copied(),
                            portrait: tile.identity.starts_with("performer:"),
                        }
                    },
                    |_| None,
                    |i| TileLabel::titled(&row.tiles[i].title, &row.tiles[i].caption),
                    |_, _, _, _| {},
                    f.cx.measure,
                );
            }
            for (c, tile) in row.tiles.iter().enumerate() {
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
                if tile.identity.starts_with("control:") || matches!(self.route, StashArg::Tags) {
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
        if !self.state.error.is_empty() {
            label(
                p,
                &self.state.error,
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
        } else if self.data.sections.iter().all(|s| s.tiles.is_empty()) {
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
            identity: format!("scene:{id}"),
            title: id.into(),
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
                title: "Scenes".into(),
                tiles: ids.iter().map(|id| tile(id)).collect(),
                portrait: false,
            }],
            ..Default::default()
        }
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
            title: "Performers".into(),
            tiles: vec![person],
            portrait: true,
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
            title: "Tag 1".into(),
            tiles: vec![tile("a")],
            portrait: false,
        });
        screen.rebuild();
        assert_ne!(screen.rows[1].keys[0], screen.rows[2].keys[0]);
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
            title: "Performers".into(),
            tiles: vec![tile("b")],
            portrait: true,
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
    fn catalog_header_controls_and_grid_clear_shared_top_chrome() {
        let mut screen = StashScreen::new(StashArg::Performers, EntryId(1));
        screen.data = data(&["a"]);
        screen.data.sections[0].portrait = true;
        screen.rebuild();
        let chip = crate::ui::widgets::CHIP_FRAME;
        assert!(TITLE_Y >= chip.y + chip.h + theme::space::SM);
        assert!(screen.rows[0].y >= TITLE_Y + TITLE_H + theme::space::SM);
        let card_top =
            screen.rows[1].y - crate::ui::card_row::heading_lift_max(&screen.rows[1].style);
        assert!(card_top > screen.rows[0].y + screen.rows[0].style.h + theme::space::MD);
        assert_eq!(screen.rows[0].style.margin_x, MARGIN_X);
    }
    #[test]
    fn tags_are_single_item_text_rows() {
        let mut screen = StashScreen::new(StashArg::Tags, EntryId(1));
        screen.data = data(&["a", "b"]);
        screen.rebuild();
        assert_eq!(screen.rows[1].tiles.len(), 1);
        assert_eq!(screen.rows[2].tiles.len(), 1);
        assert_eq!(screen.rows[1].style.h, 72.);
        assert_eq!(screen.rows[1].style.w, SCR_W - 2. * MARGIN_X);
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
