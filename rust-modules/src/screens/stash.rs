//! Stash screens compose shared CardRow geometry; Input owns every cursor.
use super::stash_registry::*;
use crate::stash::{Config, Direction, Query};
use crate::stores::stash::{PageData, Section, Tile, Work};
use crate::ui::card_row::{CardRow, RowStyle};
use crate::ui::frame::Budget;
use crate::ui::geom::{Grid, Shelf};
use crate::ui::label::Label;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::theme;
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
    url: String,
    key: String,
    slide: bool,
    interval: u32,
    slide_at: u32,
    image: usize,
    shelf_loading: bool,
    append: bool,
    media_at: u32,
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
            .bool(self.slide)
            .u32(self.interval)
            .u32(self.image as u32);
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
const CONTENT_VIEW: Rect = Rect::new(0., 310., 1920., 770.);
pub struct StashScreen {
    route: StashArg,
    entry: EntryId,
    state: State,
    data: PageData,
    rows: Vec<Row>,
    keys: HashMap<String, u32>,
    next_key: u32,
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
    fn hero_clearance(&self, row: &Row) -> f32 {
        if matches!(self.route, StashArg::Home) && row.style.h == 540. {
            crate::ui::card_row::heading_lift_max(&row.style) + 12.
        } else {
            0.
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
    pub fn new(route: StashArg, entry: EntryId) -> Self {
        let image = match &route {
            StashArg::Viewer { index, .. } => *index,
            _ => 0,
        };
        Self {
            route,
            entry,
            state: State {
                page: 1,
                interval: 5,
                image,
                ..Default::default()
            },
            data: PageData::default(),
            rows: Vec::new(),
            keys: HashMap::new(),
            next_key: 1,
        }
    }
    fn query(&self) -> Query {
        Query {
            q: self.state.query.clone(),
            page: self.state.page.max(1),
            per_page: 24,
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
        let query = if matches!(self.route, StashArg::Viewer { .. }) {
            Query {
                per_page: 1000,
                ..self.query()
            }
        } else {
            self.query()
        };
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
    fn next_shelf(&mut self, fx: &mut Effects<'_, StashHost>) {
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
        if !matches!(self.route, StashArg::Player(_) | StashArg::Viewer { .. }) {
            sections.push(Section {
                title: String::new(),
                portrait: false,
                tiles: [
                    StashArg::Home,
                    StashArg::Performers,
                    StashArg::Scenes,
                    StashArg::Galleries,
                    StashArg::Tags,
                    StashArg::Search,
                    StashArg::Settings,
                ]
                .into_iter()
                .map(|r| {
                    Self::control(
                        &format!("nav:{}", r.label()),
                        r.label().into(),
                        Action::Open(r),
                    )
                })
                .collect(),
            });
        }
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
        match self.route {
            StashArg::Settings => {
                controls = vec![
                    Self::control(
                        "url",
                        format!("Server: {}", self.state.url),
                        Action::EditUrl,
                    ),
                    Self::control(
                        "key",
                        format!(
                            "API key: {}",
                            if self.state.key.is_empty() {
                                "optional"
                            } else {
                                "••••••••"
                            }
                        ),
                        Action::EditKey,
                    ),
                    Self::control("connect", "Test and save".into(), Action::Connect),
                ];
            }
            StashArg::Player(_) => {
                controls = vec![
                    Self::control("pause", "Play / Pause".into(), Action::Pause),
                    Self::control("back", "−30 sec".into(), Action::Seek(-30)),
                    Self::control("forward", "+30 sec".into(), Action::Seek(30)),
                    Self::control("o", "O +1".into(), Action::AddO),
                    Self::control("audio", "Audio".into(), Action::Audio),
                    Self::control("subtitles", "Subtitles".into(), Action::Subtitle),
                ];
            }
            StashArg::Viewer { .. } => {
                controls = vec![
                    Self::control("previous", "Previous".into(), Action::Previous),
                    Self::control("next", "Next".into(), Action::Next),
                    Self::control(
                        "slideshow",
                        if self.state.slide {
                            "Pause slideshow"
                        } else {
                            "Start slideshow"
                        }
                        .into(),
                        Action::Slideshow,
                    ),
                    Self::control(
                        "interval",
                        format!("{} seconds", self.state.interval),
                        Action::Interval,
                    ),
                ];
            }
            _ => {
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
                if !matches!(self.route, StashArg::Home | StashArg::Scene(_))
                    && (self.data.count == 0
                        || self
                            .data
                            .sections
                            .first()
                            .is_some_and(|s| s.tiles.len() < self.data.count))
                {
                    controls.push(Self::control("more", "Next page".into(), Action::More));
                }
            }
        }
        sections.push(Section {
            title: String::new(),
            tiles: controls,
            portrait: false,
        });
        if !matches!(self.route, StashArg::Viewer { .. } | StashArg::Player(_)) {
            sections.extend(self.data.sections.clone());
        }
        let mut old_motion = std::mem::take(&mut self.rows).into_iter();
        let mut y = if matches!(self.route, StashArg::Viewer { .. } | StashArg::Player(_)) {
            880.
        } else {
            125.
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
                    margin_x: 72.,
                    ..RowStyle::EPISODE
                }
            } else if matches!(self.route, StashArg::Tags) {
                RowStyle {
                    w: 1776.,
                    h: 72.,
                    gap: 0.,
                    margin_x: 72.,
                    focus_scale: 1.02,
                    ..RowStyle::EPISODE
                }
            } else if matches!(self.route, StashArg::Home) && section_index == 2 {
                RowStyle {
                    w: 960.,
                    h: 540.,
                    gap: 36.,
                    ..RowStyle::EPISODE
                }
            } else if s.portrait {
                RowStyle {
                    w: 200.,
                    h: 285.,
                    gap: 28.,
                    ..RowStyle::HOME
                }
            } else {
                RowStyle {
                    w: 380.,
                    h: 214.,
                    gap: 28.,
                    ..RowStyle::EPISODE
                }
            };
            let chunk = if matches!(self.route, StashArg::Home) || control {
                24
            } else if matches!(self.route, StashArg::Tags) {
                1
            } else if s.portrait {
                7
            } else {
                4
            };
            let hero_clearance = if matches!(self.route, StashArg::Home) && section_index == 2 {
                crate::ui::card_row::heading_lift_max(&style) + 12.
            } else {
                0.
            };
            for (i, tiles) in s.tiles.chunks(chunk).enumerate() {
                if !control {
                    y += if i == 0 {
                        55.
                    } else if matches!(self.route, StashArg::Tags) {
                        16.
                    } else {
                        30.
                    };
                }
                y += hero_clearance;
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
                y += hero_clearance
                    + style.h
                    + if control {
                        28.
                    } else if matches!(self.route, StashArg::Tags) {
                        16.
                    } else {
                        88.
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
                group: GroupId(i as u32),
                entry: self.entry,
                extent: Rect::new(60., self.row_y(row), 1800., row.style.h),
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
        let mut images = Vec::new();
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
        if matches!(self.route, StashArg::Viewer { .. }) {
            if let Some(image) = self.data.images.get(self.state.image) {
                if let Some(url) = &image.paths.image {
                    images.push((format!("image:{}", image.id), url.clone()));
                }
            }
        }
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
                self.state.page = 1;
                self.state.append = false;
                self.load(fx);
            }
            Action::More => {
                self.state.page += 1;
                self.state.append = true;
                self.load(fx);
            }
            Action::Sort => {
                self.state.sort = (self.state.sort + 1) % 3;
                self.state.page = 1;
                self.state.append = false;
                self.load(fx);
            }
            Action::EditSearch | Action::EditUrl | Action::EditKey => {
                self.state.editing = match action {
                    Action::EditSearch => 1,
                    Action::EditUrl => 2,
                    _ => 3,
                };
                self.state.caret = match self.state.editing {
                    1 => self.state.query.len(),
                    2 => self.state.url.len(),
                    _ => self.state.key.len(),
                };
                fx.push(Fx::App(StashFx::Keyboard(true)));
            }
            Action::Connect => {
                fx.push(Fx::App(StashFx::Work(
                    Addr {
                        to: fx.from(),
                        req: RequestId(self.state.generation),
                    },
                    Work::Connect(Config {
                        server_url: self.state.url.clone(),
                        api_key: self.state.key.clone(),
                    }),
                )));
                self.state.loading = true;
            }
            Action::Previous => {
                self.state.image = self.state.image.saturating_sub(1);
            }
            Action::Next => {
                if !self.data.images.is_empty() {
                    self.state.image = (self.state.image + 1) % self.data.images.len();
                }
            }
            Action::Slideshow => {
                self.state.slide = !self.state.slide;
                self.state.slide_at = cx.tick.ms;
            }
            Action::Interval => {
                self.state.interval = match self.state.interval {
                    3 => 5,
                    5 => 10,
                    _ => 3,
                };
            }
            _ => fx.push(Fx::App(StashFx::Player(action))),
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
                    place.clip = place.clip.intersect(CONTENT_VIEW);
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
                self.state.url = cx.views.config.server_url.clone();
                self.state.key = cx.views.config.api_key.clone();
                self.rebuild();
                if !matches!(self.route, StashArg::Settings | StashArg::Player(_)) {
                    self.load(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Uncover => {
                if !self.state.loading
                    && !matches!(
                        self.route,
                        StashArg::Settings | StashArg::Viewer { .. } | StashArg::Player(_)
                    )
                {
                    self.state.page = 1;
                    self.state.append = false;
                    self.load(fx);
                }
                Handled::Yes
            }
            ScreenEvent::Async(_, StashMsg::Loaded { generation, result })
                if *generation == self.state.generation =>
            {
                self.state.loading = false;
                match result {
                    Ok(data) => {
                        if self.state.append {
                            for section in &data.sections {
                                if let Some(existing) = self
                                    .data
                                    .sections
                                    .iter_mut()
                                    .find(|s| s.title == section.title)
                                {
                                    for tile in &section.tiles {
                                        if !existing
                                            .tiles
                                            .iter()
                                            .any(|t| t.identity == tile.identity)
                                        {
                                            existing.tiles.push(tile.clone());
                                        }
                                    }
                                } else {
                                    self.data.sections.push(section.clone());
                                }
                            }
                            self.data.images.extend(data.images.clone());
                            self.data.count = data.count;
                        } else {
                            self.data = data.clone();
                        }
                        self.state.append = false;
                        self.state.image = self
                            .state
                            .image
                            .min(self.data.images.len().saturating_sub(1));
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
            ScreenEvent::Async(_, StashMsg::Connected(result)) => {
                self.state.loading = false;
                self.state.error = match result {
                    Ok(_) => crate::i18n::msg::settings_stash_connection_saved().into(),
                    Err(e) => e.clone(),
                };
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                Handled::Yes
            }
            ScreenEvent::Activate(key) => {
                self.activate(*key, cx, fx);
                Handled::Yes
            }
            ScreenEvent::PressCommit(_) => {
                if let Some(k) = cx.focus.current {
                    self.activate(k.elem, cx, fx);
                }
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } => {
                if let Some((r, _)) = self.position(to.elem) {
                    let row = &self.rows[r];
                    if Self::pinned(row) {
                    } else if row.y - self.state.scroll + row.style.h > 980. {
                        self.state.scroll = row.y + row.style.h - 980.;
                    } else if row.y - self.state.scroll < CONTENT_VIEW.y + 48. {
                        self.state.scroll = (row.y - CONTENT_VIEW.y - 48.).max(0.);
                    }
                }
                self.media(cx, fx);
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
                let value = match self.state.editing {
                    1 => &mut self.state.query,
                    2 => &mut self.state.url,
                    _ => &mut self.state.key,
                };
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
                let value = match self.state.editing {
                    1 => &self.state.query,
                    2 => &self.state.url,
                    _ => &self.state.key,
                };
                let mut buffer =
                    crate::ui::text_buffer::TextBuffer::new(value.clone(), self.state.caret);
                buffer.edit(&edit);
                self.state.caret = buffer.caret();
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        edge: Edge::Repeat,
                        key: Key::Ok,
                        ..
                    },
                ..
            }) if matches!(self.route, StashArg::Player(_)) => Handled::Yes,
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
                }
                if self.state.slide
                    && t.ms.wrapping_sub(self.state.slide_at) >= self.state.interval * 1000
                    && !self.data.images.is_empty()
                {
                    self.state.image = (self.state.image + 1) % self.data.images.len();
                    self.state.slide_at = t.ms;
                    self.media(cx, fx);
                    fx.invalidate(crate::ui::present::Provenance::Input);
                }
                Handled::Yes
            }
            ScreenEvent::WillLeave(_) | ScreenEvent::Cover | ScreenEvent::Unmount => {
                fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                if matches!(self.route, StashArg::Player(_)) {
                    fx.push(Fx::App(StashFx::Player(Action::Open(StashArg::Home))));
                }
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
        if matches!(self.route, StashArg::Player(_)) {
            RenderStrategy::VideoPlane
        } else {
            RenderStrategy::Page
        }
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let p = f.painter;
        let title = if self.data.title.is_empty() {
            self.route.label()
        } else {
            &self.data.title
        };
        label(
            p,
            title,
            Rect::new(72., 32., 1700., 65.),
            theme::size::TITLE,
            theme::TEXT_PRIMARY,
        );
        if matches!(self.route, StashArg::Viewer { .. }) {
            if let Some(i) = self.data.images.get(self.state.image) {
                if let Some((tex, w, h)) = f.cx.views.textures.get(&format!("image:{}", i.id)) {
                    let rect = Rect::new(72., 120., 1776., 720.);
                    let scale = (rect.w / *w).min(rect.h / *h);
                    let fit = Rect::new(
                        rect.cx() - *w * scale / 2.,
                        rect.cy() - *h * scale / 2.,
                        *w * scale,
                        *h * scale,
                    );
                    p.tex(*tex, fit, 0., theme::TEXT_PRIMARY);
                }
                label(
                    p,
                    &format!("{} / {}", self.state.image + 1, self.data.images.len()),
                    Rect::new(72., 100., 1776., 40.),
                    theme::size::CAPTION,
                    theme::TEXT_SECONDARY,
                );
            }
        }
        if matches!(self.route, StashArg::Player(_)) {
            label(
                p,
                f.cx.views.player_status,
                Rect::new(72., 110., 1776., 60.),
                theme::size::CAPTION,
                theme::TEXT_SECONDARY,
            );
        }
        for (r, row) in self.rows.iter().enumerate() {
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
                Some(f.clip(p, CONTENT_VIEW))
            };
            let p = if Self::pinned(row) {
                p
            } else {
                p.clipped(CONTENT_VIEW)
            };
            if !row.title.is_empty() {
                label(
                    p,
                    &row.title,
                    Rect::new(72., y - 48. - self.hero_clearance(row), 1776., 40.),
                    theme::size::HEADLINE,
                    theme::TEXT_PRIMARY,
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
                } else {
                    let tint = theme::TEXT_PRIMARY;
                    let preview_key = format!("preview:{}", tile.identity);
                    let texture = if focus {
                        f.cx.views
                            .textures
                            .get(&preview_key)
                            .or_else(|| f.cx.views.textures.get(&tile.identity))
                    } else {
                        f.cx.views.textures.get(&tile.identity)
                    };
                    if let Some((tex, w, h)) = texture {
                        p.tex_carded(
                            *tex,
                            rect.cover_uv(*w, *h, crate::ui::Crop::Centre),
                            rect,
                            theme::CARD_RING_RAD,
                            tint,
                            if focus { 1. } else { 0. },
                        );
                    } else {
                        p.rect(
                            rect,
                            theme::CARD_RING_RAD,
                            theme::CONTROL_IDLE_FILL,
                            theme::CONTROL_IDLE_FILL,
                            if focus { 1. } else { 0. },
                        );
                    }
                    let text = crate::text::elide_by(&tile.title, rect.w, false, |s| {
                        let text = std::ffi::CString::new(s.replace('\0', "")).unwrap();
                        f.cx.measure.width(&text, theme::size::CAPTION, false)
                    });
                    label(
                        p,
                        &text,
                        Rect::new(
                            rect.x,
                            y + row.style.h + self.hero_clearance(row) + 8.,
                            rect.w,
                            35.,
                        ),
                        theme::size::CAPTION,
                        theme::TEXT_PRIMARY,
                    );
                    label(
                        p,
                        &tile.caption,
                        Rect::new(
                            rect.x,
                            y + row.style.h + self.hero_clearance(row) + 42.,
                            rect.w,
                            30.,
                        ),
                        theme::size::CAPTION,
                        theme::TEXT_SECONDARY,
                    );
                }
            }
            if matches!(self.route, StashArg::Home) && r == 2 {
                let active =
                    f.cx.focus
                        .current
                        .and_then(|k| self.position(k.elem))
                        .filter(|(row, _)| *row == r)
                        .map(|(_, c)| c)
                        .or_else(|| {
                            f.cx.focus
                                .remembered(GroupId(r as u32))
                                .and_then(|key| self.position(key))
                                .map(|(_, c)| c)
                        })
                        .unwrap_or(0);
                crate::ui::widgets::PageDots::new(row.tiles.len())
                    .active(active)
                    .centered_at(1680., y + row.style.h - 30.)
                    .draw(&crate::ui::Env::inert(), p);
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
        } else if self.data.sections.iter().all(|s| s.tiles.is_empty())
            && !matches!(
                self.route,
                StashArg::Settings | StashArg::Player(_) | StashArg::Viewer { .. }
            )
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
    fn gallery_slideshow_defaults_to_five_seconds() {
        let screen = StashScreen::new(
            StashArg::Viewer {
                gallery: "1".into(),
                index: 9,
            },
            EntryId(1),
        );
        assert_eq!(screen.state.interval, 5);
        assert_eq!(screen.state.image, 9);
        assert!(!screen.state.slide);
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
        assert_ne!(screen.rows[2].keys[0], screen.rows[3].keys[0]);
    }
    #[test]
    fn initial_query_is_first_page_and_performer_sort_descending() {
        let screen = StashScreen::new(StashArg::Performers, EntryId(1));
        let query = screen.query();
        assert_eq!(query.page, 1);
        assert_eq!(query.sort, "o_counter");
        assert_eq!(query.direction, Direction::Descending);
    }
    #[test]
    fn home_newest_scenes_use_hero_carousel_geometry() {
        let mut screen = StashScreen::new(StashArg::Home, EntryId(1));
        screen.data = data(&["a", "b"]);
        screen.rebuild();
        assert_eq!(screen.rows[2].style.w, 960.);
        assert_eq!(screen.rows[2].style.h, 540.);
        assert_eq!(screen.rows[2].tiles.len(), 2);
    }
    #[test]
    fn hero_focus_growth_clears_heading_and_caption() {
        let mut screen = StashScreen::new(StashArg::Home, EntryId(1));
        screen.data = data(&["a"]);
        screen.rebuild();
        let row = &screen.rows[2];
        let clearance = screen.hero_clearance(row);
        let pop = crate::ui::card_row::heading_lift_max(&row.style);
        let heading_bottom = row.y - 48. - clearance + 40.;
        let focused_top = row.y - pop;
        let focused_bottom = row.y + row.style.h + pop;
        let caption_top = row.y + row.style.h + clearance + 8.;
        assert!(focused_top - heading_bottom >= 19.9);
        assert!(caption_top - focused_bottom >= 19.9);
        assert_eq!(screen.hero_clearance(&screen.rows[0]), 0.);
    }
    #[test]
    fn browsing_scroll_keeps_controls_pinned_and_omits_duplicate_heading() {
        let mut screen = StashScreen::new(StashArg::Scenes, EntryId(1));
        screen.data = data(&["a", "b", "c", "d", "e"]);
        screen.rebuild();
        let nav_y = screen.row_y(&screen.rows[0]);
        let content_y = screen.row_y(&screen.rows[2]);
        screen.state.scroll = 250.;
        assert_eq!(screen.row_y(&screen.rows[0]), nav_y);
        assert_eq!(screen.row_y(&screen.rows[2]), content_y - 250.);
        assert!(screen.rows[2].title.is_empty());
        screen.grid(|grid| assert_eq!(grid.shelves[0].group, GroupId(0)));
    }
    #[test]
    fn tags_are_single_item_text_rows() {
        let mut screen = StashScreen::new(StashArg::Tags, EntryId(1));
        screen.data = data(&["a", "b"]);
        screen.rebuild();
        assert_eq!(screen.rows[2].tiles.len(), 1);
        assert_eq!(screen.rows[3].tiles.len(), 1);
        assert_eq!(screen.rows[2].style.h, 72.);
        assert_eq!(screen.rows[2].style.w, 1776.);
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
                player_status: "",
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
    #[test]
    fn slideshow_advances_at_interval_and_stops_after_pause() {
        let _lock = crate::testlock::serial();
        let config = Config::default();
        let textures = HashMap::new();
        let mut cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                player_status: "",
            },
            tick: Tick::default(),
            measure: &Measure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut screen = StashScreen::new(
            StashArg::Viewer {
                gallery: "g".into(),
                index: 0,
            },
            EntryId(1),
        );
        screen.data.images = vec![
            crate::stash::Image::default(),
            crate::stash::Image::default(),
        ];
        screen.rebuild();
        let toggle = screen.keys["control:slideshow"];
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        screen.activate(toggle, &cx, &mut fx);
        cx.tick.ms = 4999;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.state.image, 0);
        cx.tick.ms = 5000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.state.image, 1);
        screen.activate(toggle, &cx, &mut fx);
        cx.tick.ms = 10000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.state.image, 1);
    }
}
