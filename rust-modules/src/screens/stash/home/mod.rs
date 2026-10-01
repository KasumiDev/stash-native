//! Stash landing page: original billboard and hero/shelf doors over Stash publications.
use crate::screens::stash::catalog::StashScreen;
use crate::screens::stash_registry::*;
use crate::ui::geom::TabRow;
use crate::ui::icons::Icon;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::widgets::{Button, CircleButton, PageDots};
use crate::ui::{landing_hero, theme};
use crate::ui::{Env, Rect, Spring, View};
use std::borrow::Cow;
const HERO: u32 = 0x100000;
const GROUP: GroupId = GroupId(0);
const SNAP_EXTENT: f32 = crate::ui::consts::PEEK_Y - crate::ui::consts::GRID_TOP_Y;
pub struct HomeScreen {
    shelves: StashScreen,
    entry: EntryId,
    index: usize,
    selected: Option<String>,
    snap: Spring,
    target: f32,
    scroll: Spring,
    scroll_target: f32,
    auto_at: u32,
    covered: bool,
}
impl HomeScreen {
    pub fn new(entry: EntryId) -> Self {
        Self {
            shelves: StashScreen::home(entry),
            entry,
            index: 0,
            selected: None,
            snap: Spring::at(0.),
            target: 0.,
            scroll: Spring::at(0.),
            scroll_target: 0.,
            auto_at: 0,
            covered: false,
        }
    }
    fn scene(&self) -> Option<&crate::stash::Scene> {
        self.shelves.scenes().get(self.index)
    }
    fn play_label(&self) -> &'static std::ffi::CStr {
        if self.shelves.loading() {
            crate::i18n::msg::browse_home_loading_c()
        } else if self.scene().is_none() {
            crate::i18n::msg::browse_action_retry_c()
        } else if self.scene().is_some_and(|s| s.resume_time > 0.) {
            crate::i18n::msg::browse_home_continue_c()
        } else {
            crate::i18n::msg::browse_detail_play_c()
        }
    }
    fn rects(&self, cx: &Cx<'_, StashHost>) -> [Rect; 2] {
        let label = self.play_label();
        let w = cx.measure.width(label, theme::size::BODY, true) + 96.;
        let y = landing_hero::TEXT_BOTTOM + theme::space::MD - self.snap.pos * SNAP_EXTENT;
        [
            Rect::new(96., y, w, 60.),
            Rect::new(96. + w + 16., y, 60., 60.),
        ]
    }
    fn hero<R>(&self, cx: &Cx<'_, StashHost>, f: impl FnOnce(TabRow<'_>) -> R) -> R {
        let rects = self.rects(cx);
        f(TabRow {
            rects: &rects[..if self.scene().is_some() { 2 } else { 1 }],
            group: GROUP,
            entry: self.entry,
        })
    }
    fn flip(&mut self, delta: i32, now: u32) {
        let n = self.shelves.scenes().len();
        if n > 0 {
            self.index = (self.index as i32 + delta).rem_euclid(n as i32) as usize;
            self.selected = self.scene().map(|s| s.id.clone());
            self.auto_at = now;
        }
    }
    fn activate(&mut self, key: u32, _cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        if let Some(arg) = strip_destination(key) {
            fx.push(Fx::Nav(NavOp::Root(arg)));
            return;
        }
        if key == HERO && (self.shelves.loading() || self.scene().is_none()) {
            self.shelves.retry(fx);
            fx.invalidate(crate::ui::present::Provenance::Input);
            return;
        }
        match key.checked_sub(HERO) {
            Some(0) => {
                if let Some(scene) = self.scene() {
                    fx.push(Fx::App(StashFx::Play(
                        scene.clone(),
                        scene.resume_time > 0.,
                    )));
                    fx.push(Fx::Nav(NavOp::Push(StashArg::Player(scene.id.clone()))));
                }
            }
            Some(1) => {
                if let Some(scene) = self.scene() {
                    fx.push(Fx::Nav(NavOp::Push(StashArg::Scene(scene.id.clone()))));
                }
            }
            _ => {}
        }
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
}
impl LogicalState for HomeScreen {
    fn write(&self, c: &mut Canon) {
        self.shelves.state().write(c);
        c.u32(self.index as u32).f32(self.target).bool(self.covered);
    }
    fn probe(&self, s: &mut String) {
        self.shelves.state().probe(s);
        s.push_str(&format!(" hero={} snap={}", self.index, self.target));
    }
}
impl Focusable<StashHost> for HomeScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        self.hero(cx, |g| g.groups(cx, out));
        self.shelves.groups(cx, out);
    }
    fn group_of(&self, key: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        if (HERO..HERO + 2).contains(key) {
            Some(GROUP)
        } else {
            self.shelves.group_of(key, cx)
        }
    }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        if (HERO..HERO + 2).contains(&key.elem) {
            self.hero(cx, |g| {
                match g.neighbour(
                    FocusKey {
                        entry: key.entry,
                        elem: key.elem - HERO,
                    },
                    dir,
                    cx,
                ) {
                    Step::Move(k) => Step::Move(FocusKey {
                        entry: k.entry,
                        elem: k.elem + HERO,
                    }),
                    v => v,
                }
            })
        } else {
            self.shelves.neighbour(key, dir, cx)
        }
    }
    fn place(&self, key: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        if (HERO..HERO + 2).contains(key) {
            self.hero(cx, |g| g.place(&(*key - HERO), cx, at))
        } else {
            self.shelves.place(key, cx, at)
        }
    }
    fn reconcile(&self, key: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if (HERO..HERO + 2).contains(&key.elem) {
            key
        } else {
            self.shelves.reconcile(key, cx)
        }
    }
    fn seat(&self, group: GroupId, from: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if group == GROUP {
            self.hero(cx, |g| {
                let k = g.seat(group, from, cx);
                FocusKey {
                    entry: k.entry,
                    elem: k.elem + HERO,
                }
            })
        } else {
            self.shelves.seat(group, from, cx)
        }
    }
}
impl Machine<StashHost> for HomeScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        event: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        match event {
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key,
                        edge: Edge::Down | Edge::Repeat,
                        at_edge,
                        ..
                    },
                ..
            }) if *at_edge
                && matches!(key, Key::Left | Key::Right)
                && cx
                    .focus
                    .current
                    .is_some_and(|k| (HERO..HERO + 2).contains(&k.elem)) =>
            {
                self.flip(if *key == Key::Left { -1 } else { 1 }, cx.tick.ms);
                fx.invalidate(crate::ui::present::Provenance::Input);
                self.shelves.media_override(Vec::new(), self.hero_preview());
                return Handled::Yes;
            }
            ScreenEvent::Input(_) => self.auto_at = cx.tick.ms,
            ScreenEvent::Activate(key) if *key >= HERO => {
                self.activate(*key, cx, fx);
                return Handled::Yes;
            }
            ScreenEvent::PressCommit(_) if cx.focus.current.is_some_and(|k| k.elem >= HERO) => {
                self.activate(cx.focus.current.unwrap().elem, cx, fx);
                return Handled::Yes;
            }
            ScreenEvent::FocusMoved { to, .. } => {
                self.target = if self.shelves.group_of(&to.elem, cx).is_some() {
                    1.
                } else {
                    0.
                };
                self.scroll_target = self.shelves.home_scroll_target(to.elem).unwrap_or(0.);
                self.auto_at = cx.tick.ms;
            }
            ScreenEvent::Cover => self.covered = true,
            ScreenEvent::Uncover => {
                self.covered = false;
                self.auto_at = cx.tick.ms;
            }
            ScreenEvent::Tick(t) => {
                self.snap
                    .step(self.target, crate::ui::consts::K_SNAP, t.dt());
                self.scroll
                    .step(self.scroll_target, crate::ui::consts::K_SNAP, t.dt());
                self.shelves.shelf_scroll(self.scroll.pos);
                if !self.covered && self.target < 0.5 && t.ms.wrapping_sub(self.auto_at) >= 8000 {
                    self.flip(1, t.ms);
                    fx.invalidate(crate::ui::present::Provenance::Input);
                }
                if (self.snap.pos - self.target).abs() > 0.002
                    || self.snap.vel.abs() > 0.01
                    || (self.scroll.pos - self.scroll_target).abs() > 0.5
                    || self.scroll.vel.abs() > 0.5
                {
                    fx.invalidate(crate::ui::present::Provenance::Input);
                }
            }
            _ => {}
        }
        self.shelves.media_override(Vec::new(), self.hero_preview());
        let result = self.shelves.step(event, cx, fx);
        if matches!(result, Handled::Yes)
            && matches!(event, ScreenEvent::Async(_, StashMsg::Loaded { .. }))
        {
            self.index = self
                .selected
                .as_ref()
                .and_then(|id| self.shelves.scenes().iter().position(|s| &s.id == id))
                .unwrap_or(0);
            self.selected = self.scene().map(|s| s.id.clone());
            self.auto_at = cx.tick.ms;
        }
        result
    }
}
impl HomeScreen {
    fn hero_preview(&self) -> Option<(String, String)> {
        if self.covered || self.target >= 0.5 {
            return None;
        }
        self.scene().and_then(|s| {
            s.paths
                .preview
                .as_ref()
                .map(|url| (format!("hero:{}", s.id), url.clone()))
        })
    }
}
impl Screen<StashHost> for HomeScreen {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn name(&self) -> &'static str {
        "StashHome"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, b: &mut crate::ui::frame::Budget, cx: &Cx<'_, StashHost>) {
        self.shelves.prepare(b, cx);
    }
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn strip_reachable(&self) -> bool {
        self.snap.pos < 0.5
    }
    fn links(&self, out: &mut Vec<Link>) {
        let strip = crate::ui::containers::tabs::STRIP;
        out.push(Link {
            from: strip,
            dir: Dir::Down,
            to: GROUP,
        });
        out.push(Link {
            from: GROUP,
            dir: Dir::Up,
            to: strip,
        });
        if let Some(first) = self.shelves.first_group() {
            out.push(Link {
                from: GROUP,
                dir: Dir::Down,
                to: first,
            });
            out.push(Link {
                from: first,
                dir: Dir::Up,
                to: GROUP,
            });
        }
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let a = crate::ui::hero_alpha(self.snap.pos, 0.8);
        let p = f.painter;
        if let Some(scene) = self.scene() {
            if let Some((tex, w, h)) =
                f.cx.views
                    .textures
                    .get(&format!("preview:hero:{}", scene.id))
                    .or_else(|| f.cx.views.textures.get(&format!("hero:{}", scene.id)))
            {
                p.alpha(a).tex_uv(
                    *tex,
                    Rect::FULL.cover_uv(*w, *h, crate::ui::Crop::Centre),
                    Rect::FULL,
                    0.,
                    theme::TEXT_PRIMARY,
                );
            }
            let hero = p.alpha(a);
            crate::ui::widgets::hero_scrim(hero, a, false);
            let [y0, knee, mid, foot] = landing_hero::base_scrim_ramp(a);
            hero.grad4(
                Rect::new(0., y0, 1920., knee - y0),
                [
                    [0., 0., 0., 0.],
                    [0., 0., 0., 0.],
                    [0., 0., 0., mid],
                    [0., 0., 0., mid],
                ],
            );
            hero.grad4(
                Rect::new(0., knee, 1920., 1080. - knee),
                [
                    [0., 0., 0., mid],
                    [0., 0., 0., mid],
                    [0., 0., 0., foot],
                    [0., 0., 0., foot],
                ],
            );
            let title = crate::ui::text_view::TextView::new(
                scene.display_title(),
                theme::size::HERO,
                theme::TEXT_PRIMARY,
            )
            .max_lines(2);
            let names = scene
                .performers
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let meta = format!(
                "{} · {} · {}",
                scene.date.as_deref().unwrap_or(""),
                names,
                crate::i18n::msg::stash_player_count(scene.o_counter)
            );
            let meta = crate::ui::text_view::TextView::new(
                &meta,
                theme::size::BODY,
                theme::TEXT_SECONDARY,
            )
            .max_lines(1);
            let synopsis = crate::ui::hero_synopsis(scene.details.as_deref().unwrap_or(""), "");
            let title_h = title.measure_h(landing_hero::COL_W);
            let meta_h = meta.measure_h(landing_hero::COL_W);
            let synopsis_h = synopsis.measure_h(landing_hero::COL_W);
            let y = landing_hero::stack_top(title_h, meta_h, synopsis_h + theme::space::SM)
                - self.snap.pos * SNAP_EXTENT;
            title.draw(hero, Rect::new(96., y, landing_hero::COL_W, 0.));
            meta.draw(
                hero,
                Rect::new(96., y + title_h + theme::space::MD, landing_hero::COL_W, 0.),
            );
            synopsis.draw(
                hero,
                Rect::new(
                    96.,
                    y + title_h + theme::space::MD + meta_h + theme::space::SM,
                    landing_hero::COL_W,
                    0.,
                ),
            );
        }
        let hero = p.alpha(a);
        let env = Env::inert();
        let rects = self.rects(f.cx);
        let label = self.play_label();
        let count = if self.scene().is_some() { 2 } else { 1 };
        for (i, rect) in rects.into_iter().take(count).enumerate() {
            let key = FocusKey {
                entry: self.entry,
                elem: HERO + i as u32,
            };
            let focused = f.cx.focus.current == Some(key);
            f.stop(
                p,
                Stop {
                    key,
                    rect,
                    rest_rect: rect,
                    clip: Rect::FULL,
                    hover: Hover::Focus,
                    activate: Activate::Press,
                },
            );
            if i == 0 {
                Button::new(label.as_ptr(), theme::size::BODY, rect)
                    .icon(Icon::Play)
                    .focused(focused)
                    .draw(&env, hero);
            } else {
                let icon = Icon::Info;
                CircleButton::new(c"".as_ptr())
                    .icon(icon)
                    .frame(rect)
                    .focused(focused)
                    .draw(&env, hero);
            }
        }
        if !self.shelves.scenes().is_empty() {
            PageDots::new(self.shelves.scenes().len())
                .active(self.index)
                .centered_at(960., rects[0].y + 84.)
                .draw(&env, hero);
        }
        self.shelves.draw(f);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::stash::{PageData, Section, Tile};
    fn publication() -> PageData {
        let scenes = (1..=12)
            .map(|i| crate::stash::Scene {
                id: i.to_string(),
                title: Some(format!("Scene {i}")),
                details: Some("Synthetic synopsis".into()),
                paths: crate::stash::ScenePaths {
                    preview: Some(format!("fixture://preview/{i}")),
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect::<Vec<_>>();
        PageData {
            title: "Home".into(),
            scenes,
            sections: vec![
                Section {
                    title: "Newest scenes".into(),
                    portrait: false,
                    shelf: true,
                    tiles: vec![Tile {
                        identity: "scene:1".into(),
                        title: "Scene 1".into(),
                        caption: String::new(),
                        image: None,
                        preview: None,
                        action: Action::Open(StashArg::Scene("1".into())),
                    }],
                },
                Section {
                    title: "Performers".into(),
                    portrait: true,
                    shelf: true,
                    tiles: vec![Tile {
                        identity: "performer:1".into(),
                        title: "Favorite performer".into(),
                        caption: "★ · O 10".into(),
                        image: None,
                        preview: None,
                        action: Action::Open(StashArg::Performer("1".into())),
                    }],
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn billboard_loads_raw_scene_metadata_and_projects_only_remaining_shelves() {
        let _lock = crate::testlock::serial();
        let config = crate::stash::Config::default();
        let textures = std::collections::HashMap::new();
        let playback = PlaybackView::default();
        let cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: &playback,
            },
            tick: Tick::default(),
            measure: &crate::ui::fixture::FixtureMeasure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut home = HomeScreen::new(EntryId(1));
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        home.step(&ScreenEvent::Mount, &cx, &mut fx);
        home.step(
            &ScreenEvent::Async(
                RequestId(1),
                StashMsg::Loaded {
                    generation: 1,
                    result: Ok(publication()),
                },
            ),
            &cx,
            &mut fx,
        );
        assert_eq!(home.shelves.scenes().len(), 12);
        assert_eq!(
            home.scene().unwrap().details.as_deref(),
            Some("Synthetic synopsis")
        );
        assert!(home.shelves.test_content_key("scene:1").is_none());
        assert!(home.shelves.test_content_key("performer:1").is_some());
        assert!(home.place(&HERO, &cx, At::Drawn).is_some());
        assert!(home.place(&(HERO + 1), &cx, At::Drawn).is_some());
        assert!(home.place(&(HERO + 2), &cx, At::Drawn).is_none());
        assert!(matches!(
            home.neighbour(
                FocusKey {
                    entry: EntryId(1),
                    elem: HERO + 1
                },
                Dir::Right,
                &cx
            ),
            Step::Edge
        ));
        assert_eq!(
            home.hero_preview(),
            Some(("hero:1".into(), "fixture://preview/1".into()))
        );
        home.flip(-1, 20);
        assert_eq!(home.scene().unwrap().id, "12");
        home.flip(1, 30);
        assert_eq!(home.scene().unwrap().id, "1");
        home.covered = true;
        assert_eq!(home.hero_preview(), None);
        home.covered = false;
        home.target = 1.;
        assert_eq!(home.hero_preview(), None);
    }
    #[test]
    fn failed_billboard_load_retains_an_actionable_retry() {
        let _lock = crate::testlock::serial();
        let config = crate::stash::Config::default();
        let textures = std::collections::HashMap::new();
        let playback = PlaybackView::default();
        let cx = Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: &playback,
            },
            tick: Tick::default(),
            measure: &crate::ui::fixture::FixtureMeasure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        };
        let mut home = HomeScreen::new(EntryId(1));
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        home.step(&ScreenEvent::Mount, &cx, &mut fx);
        home.step(
            &ScreenEvent::Async(
                RequestId(1),
                StashMsg::Loaded {
                    generation: 1,
                    result: Err("Synthetic failure".into()),
                },
            ),
            &cx,
            &mut fx,
        );
        assert_eq!(home.play_label(), crate::i18n::msg::browse_action_retry_c());
        assert!(home.place(&HERO, &cx, At::Drawn).is_some());
        home.activate(HERO, &cx, &mut fx);
        assert!(home.shelves.loading());
    }
}
