//! Stash transport composed from the original shared video-plane HUD.
mod markers;
use crate::screens::stash_registry::*;
use crate::ui::frame::Budget;
use crate::ui::machine::*;
use crate::ui::player_hud::{self, Knob, Playbar, TransportMark};
use crate::ui::screen::*;
use crate::ui::widgets::{Button, ControlGround};
use crate::ui::{theme, Env, Rect, View};
use std::{borrow::Cow, ffi::CString};

const LINGER: u32 = 4500;
#[derive(Default)]
struct State {
    until: u32,
    dismissed: bool,
    o_last: Option<u32>,
    suppressed_preview: Option<u32>,
    scrub: Option<f64>,
    completed: bool,
    expanded: bool,
    markers_loaded: bool,
}
impl LogicalState for State {
    fn write(&self, c: &mut Canon) {
        c.u32(self.until)
            .bool(self.dismissed)
            .u32(self.o_last.unwrap_or(u32::MAX))
            .u32(self.suppressed_preview.unwrap_or(0))
            .bool(self.completed)
            .bool(self.expanded)
            .bool(self.markers_loaded)
            .f32(self.scrub.unwrap_or(-1.) as f32);
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!(
            "completed={} expanded={} dismissed={}",
            self.completed, self.expanded, self.dismissed
        ));
    }
}
pub struct PlayerScreen {
    id: String,
    entry: EntryId,
    state: State,
    subtitles: player_hud::SubtitleBitmaps,
    markers: markers::Markers,
    offset: crate::ui::Spring,
}
impl PlayerScreen {
    pub fn new(id: String, entry: EntryId) -> Self {
        Self {
            id,
            entry,
            state: State::default(),
            subtitles: player_hud::SubtitleBitmaps::new(),
            markers: markers::Markers::default(),
            offset: crate::ui::Spring::at(0.),
        }
    }
    fn visible(&self, cx: &Cx<'_, StashHost>) -> bool {
        cx.views.playback.completed
            || self.state.expanded
            || cx.views.playback.loading
            || (!self.state.dismissed
                && (!cx.views.playback.playing || cx.tick.ms < self.state.until))
    }
    fn reveal(&mut self, now: u32) {
        self.state.until = now.saturating_add(LINGER);
        self.state.dismissed = false;
    }
    fn keys(&self, cx: &Cx<'_, StashHost>) -> Vec<u32> {
        if cx.views.playback.completed {
            vec![10, 11]
        } else if self.visible(cx) {
            let mut keys = vec![0, 3];
            if self.state.expanded {
                keys.extend_from_slice(self.markers.keys());
            }
            keys
        } else {
            Vec::new()
        }
    }
    fn rect(&self, key: u32, cx: &Cx<'_, StashHost>) -> Option<Rect> {
        if !self.keys(cx).contains(&key) {
            return None;
        }
        if self.markers.contains(key) {
            return self.markers.rect(key, self.offset.pos);
        }
        let mut rect = match key {
            0 => player_hud::scrub_hit_rect(),
            3 => {
                let r = player_hud::disc_hit_rect(2);
                Rect::new(r.x - 24., r.y, 112., r.h)
            }
            10 => Rect::new(670., 620., 260., 64.),
            11 => Rect::new(970., 620., 260., 64.),
            _ => return None,
        };
        if key <= 3 {
            rect.y -= self.offset.pos;
        }
        Some(rect)
    }
    fn emit(&self, action: Action, fx: &mut Effects<'_, StashHost>) {
        fx.push(Fx::App(StashFx::Player(action)));
    }
    fn activate(&mut self, key: u32, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        self.reveal(cx.tick.ms);
        if let Some(seconds) = self.markers.seconds(key) {
            self.state.scrub = None;
            self.state.suppressed_preview = Some(key);
            fx.push(Fx::App(StashFx::Media(
                self.markers.images(self.offset.pos),
                None,
            )));
            self.emit(Action::SeekTo(seconds), fx);
            return;
        }
        match key {
            0 if !cx.views.playback.completed => self.emit(Action::Pause, fx),
            3 | 11
                if !cx.views.playback.o_pending
                    && self
                        .state
                        .o_last
                        .is_none_or(|last| cx.tick.ms.wrapping_sub(last) >= 2000) =>
            {
                self.state.o_last = Some(cx.tick.ms);
                self.emit(Action::AddO, fx);
            }
            10 => {
                self.state.completed = false;
                self.emit(Action::Replay, fx);
            }
            _ => {}
        }
    }
}
impl Focusable<StashHost> for PlayerScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        let keys = self.keys(cx);
        if keys.is_empty() {
            return;
        }
        out.push(GroupSpec {
            id: GroupId(0),
            kind: GroupKind::Free,
            seat: Seat::First,
            reachable: AxisMask::BOTH,
            edge: [EdgeRule::Screen; 4],
            extent: Rect::FULL,
            len: keys.len(),
            elem: ElemKind::Control,
        });
    }
    fn group_of(&self, k: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        self.rect(*k, cx).map(|_| GroupId(0))
    }
    fn neighbour(&self, k: FocusKey<u32>, d: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        let keys = self.keys(cx);
        let Some(i) = keys.iter().position(|x| *x == k.elem) else {
            return Step::Edge;
        };
        let next = if !cx.views.playback.completed {
            if self.markers.contains(k.elem) {
                match d {
                    Dir::Left => self.markers.neighbour(k.elem, false),
                    Dir::Right => self.markers.neighbour(k.elem, true),
                    Dir::Up => Some(3),
                    Dir::Down => None,
                }
            } else {
                match d {
                    Dir::Down if k.elem == 0 => self.markers.near(cx.views.playback.position),
                    Dir::Down if k.elem == 3 => Some(0),
                    Dir::Up if k.elem == 0 => Some(3),
                    Dir::Up => None,
                    _ => None,
                }
            }
        } else {
            match d {
                Dir::Left | Dir::Up => i.checked_sub(1).map(|i| keys[i]),
                Dir::Right | Dir::Down => keys.get(i + 1).copied(),
            }
        };
        next.map(|elem| {
            Step::Move(FocusKey {
                entry: self.entry,
                elem,
            })
        })
        .unwrap_or(Step::Edge)
    }

    fn place(&self, k: &u32, cx: &Cx<'_, StashHost>, _: At) -> Option<Placed> {
        let r = self.rect(*k, cx)?;
        Some(Placed {
            rect: r,
            rest_rect: r,
            clip: Rect::FULL,
            index: Some(*k),
        })
    }
    fn reconcile(&self, k: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if self.rect(k.elem, cx).is_some() {
            k
        } else {
            FocusKey {
                entry: self.entry,
                elem: self.keys(cx).first().copied().unwrap_or(0),
            }
        }
    }
    fn seat(&self, _: GroupId, _: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: self.keys(cx).first().copied().unwrap_or(0),
        }
    }
}
impl Machine<StashHost> for PlayerScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        ev: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        let handled = match ev {
            ScreenEvent::Unmount | ScreenEvent::Suspend | ScreenEvent::Cover => {
                fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                self.state.suppressed_preview = None;
                self.subtitles.release();
                Handled::Yes
            }
            ScreenEvent::Mount => {
                self.reveal(cx.tick.ms);
                self.markers.sync(
                    cx.views
                        .playback
                        .scene
                        .as_ref()
                        .map_or(&[], |s| s.scene_markers.as_slice()),
                );
                fx.push(Fx::App(StashFx::Work(
                    Addr {
                        to: fx.from(),
                        req: RequestId(1),
                    },
                    crate::stores::stash::Work::Load {
                        route: StashArg::Scene(self.id.clone()),
                        query: crate::stash::Query::default(),
                        generation: 1,
                    },
                )));
                Handled::Yes
            }
            ScreenEvent::Async(
                _,
                StashMsg::Loaded {
                    generation: 1,
                    result: Ok(data),
                },
            ) => {
                if let Some(scene) = data.scene.as_ref().filter(|s| s.id == self.id) {
                    self.markers.sync(&scene.scene_markers);
                    self.state.markers_loaded = true;
                }
                Handled::Yes
            }
            ScreenEvent::Uncover => {
                self.reveal(cx.tick.ms);
                Handled::Yes
            }
            ScreenEvent::Tick(t) => {
                if !self.state.markers_loaded {
                    self.markers.sync(
                        cx.views
                            .playback
                            .scene
                            .as_ref()
                            .map_or(&[], |s| s.scene_markers.as_slice()),
                    );
                }
                if self.markers.empty() {
                    self.state.expanded = false;
                }
                self.offset.step(
                    if self.state.expanded && self.visible(cx) && !cx.views.playback.completed {
                        markers::SHIFT
                    } else {
                        0.
                    },
                    220.,
                    t.dt(),
                );
                self.markers
                    .update(cx.focus.current.map(|k| k.elem), t.dt());
                fx.push(Fx::App(StashFx::Media(
                    if self.state.expanded {
                        self.markers.images(self.offset.pos)
                    } else {
                        Vec::new()
                    },
                    if self.state.expanded && self.visible(cx) && !cx.views.playback.completed {
                        let focused = cx.focus.current.map(|k| k.elem);
                        self.markers
                            .preview(focused.filter(|k| Some(*k) != self.state.suppressed_preview))
                    } else {
                        None
                    },
                )));
                if cx.views.playback.completed && !self.state.completed {
                    self.state.completed = true;
                    self.state.expanded = false;
                    self.state.scrub = None;
                }
                Handled::Yes
            }
            ScreenEvent::Activate(k) => {
                self.activate(*k, cx, fx);
                Handled::Yes
            }
            ScreenEvent::PressCommit(_) => {
                if let Some(key) = cx.focus.current.filter(|k| k.entry == self.entry) {
                    if self.keys(cx).contains(&key.elem) {
                        self.activate(key.elem, cx, fx);
                    }
                }
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } => {
                if self.state.suppressed_preview != Some(to.elem) {
                    self.state.suppressed_preview = None;
                }
                // Hiding removes the focus stops. The dispatcher's reconciliation
                // must not reopen the HUD as it seats the now-hidden timeline.
                if self.visible(cx) {
                    self.state.expanded = self.markers.contains(to.elem);
                    self.reveal(cx.tick.ms);
                }
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Ok,
                        edge: Edge::Repeat,
                        ..
                    },
                ..
            }) => Handled::Yes,
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Back,
                        edge: Edge::Down,
                        ..
                    },
                ..
            }) => {
                if self.state.expanded {
                    self.state.expanded = false;
                    self.state.suppressed_preview = None;
                    fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                    self.reveal(cx.tick.ms);
                } else {
                    self.emit(Action::Open(StashArg::Scene(self.id.clone())), fx);
                    fx.push(Fx::Nav(NavOp::Pop));
                }
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key,
                        edge: Edge::Down | Edge::Repeat,
                        ..
                    },
                ..
            }) if matches!(key, Key::Left | Key::Right | Key::Up | Key::Down | Key::Ok) => {
                if !self.visible(cx) {
                    self.reveal(cx.tick.ms);
                    fx.push(Fx::Deliver(
                        fx.from(),
                        Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                            focus: FocusTarget::Elem(FocusKey {
                                entry: self.entry,
                                elem: 0,
                            }),
                        })),
                    ));
                    Handled::Yes
                } else if !cx.views.playback.completed
                    && *key == Key::Up
                    && cx.focus.current.is_some_and(|k| k.elem == 3)
                {
                    self.state.expanded = false;
                    self.state.dismissed = true;
                    self.state.scrub = None;
                    self.state.suppressed_preview = None;
                    fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
                    Handled::Yes
                } else if !cx.views.playback.completed
                    && *key == Key::Down
                    && cx.focus.current.is_some_and(|k| k.elem == 0)
                    && !self.markers.empty()
                {
                    self.state.expanded = true;
                    self.reveal(cx.tick.ms);
                    Handled::No
                } else if !cx.views.playback.completed
                    && matches!(key, Key::Left | Key::Right)
                    && cx.focus.current.is_some_and(|k| k.elem == 0)
                {
                    let delta = if *key == Key::Left { -10. } else { 10. };
                    let position = (self.state.scrub.unwrap_or(cx.views.playback.position) + delta)
                        .clamp(0., cx.views.playback.duration.max(0.));
                    self.state.scrub = Some(position);
                    self.reveal(cx.tick.ms);
                    Handled::Yes
                } else {
                    Handled::No
                }
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Left | Key::Right,
                        edge: Edge::Up,
                        ..
                    },
                ..
            }) => {
                if let Some(p) = self.state.scrub.take() {
                    self.emit(Action::SeekTo(p), fx);
                }
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Other,
                        sym,
                        wcode,
                        edge: Edge::Down,
                        ..
                    },
                ..
            }) => {
                use crate::ui::consts::Key as TransportKey;
                let key = crate::ui::consts::classify(*sym, *wcode);
                match key {
                    TransportKey::Play if !cx.views.playback.completed => {
                        self.emit(Action::SetPaused(false), fx);
                        self.reveal(cx.tick.ms);
                        Handled::Yes
                    }
                    TransportKey::Pause if !cx.views.playback.completed => {
                        self.emit(Action::SetPaused(true), fx);
                        self.reveal(cx.tick.ms);
                        Handled::Yes
                    }
                    TransportKey::PlayPause if !cx.views.playback.completed => {
                        self.emit(Action::Pause, fx);
                        self.reveal(cx.tick.ms);
                        Handled::Yes
                    }
                    _ => Handled::No,
                }
            }
            ScreenEvent::Input(InputEvent {
                kind: InputKind::Pointer { .. },
                ..
            }) => {
                self.reveal(cx.tick.ms);
                Handled::No
            }
            ScreenEvent::Input(InputEvent {
                kind: InputKind::Drag {
                    x, hit: Some(0), ..
                },
                ..
            }) => {
                self.state.scrub =
                    Some(player_hud::scrub_frac_x(*x) as f64 * cx.views.playback.duration);
                self.reveal(cx.tick.ms);
                Handled::Yes
            }
            ScreenEvent::Input(InputEvent {
                kind: InputKind::Click {
                    x, hit: Some(0), ..
                },
                ..
            }) => {
                self.state.scrub = None;
                self.emit(
                    Action::SeekTo(
                        player_hud::scrub_frac_x(*x) as f64 * cx.views.playback.duration,
                    ),
                    fx,
                );
                self.reveal(cx.tick.ms);
                Handled::Yes
            }
            _ => Handled::No,
        };
        if handled == Handled::Yes {
            fx.invalidate(crate::ui::present::Provenance::Input);
        }
        handled
    }
}
fn text(p: crate::ui::Painter, s: &str, r: Rect, size: i32) {
    let s = CString::new(s.replace('\0', "")).unwrap_or_default();
    crate::ui::label::Label::new(s.as_ptr(), size, theme::TEXT_PRIMARY).draw(p, r);
}
impl Screen<StashHost> for PlayerScreen {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn name(&self) -> &'static str {
        "player"
    }
    fn state(&self) -> &dyn LogicalState {
        &self.state
    }
    fn crumb(&self, _: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, _: &mut Budget, _: &Cx<'_, StashHost>) {}
    fn render(&self) -> RenderStrategy {
        RenderStrategy::VideoPlane
    }
    fn render_report(&self) -> crate::ui::frame::RenderReport {
        self.subtitles.render_report()
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let view = f.cx.views.playback;
        let p = f.painter;
        if view.completed {
            p.rect(
                Rect::FULL,
                0.,
                theme::scrim_black(1.),
                theme::scrim_black(1.),
                0.,
            );
            text(
                p,
                crate::i18n::msg::stash_player_finished(),
                Rect::new(670., 360., 800., 70.),
                theme::size::TITLE,
            );
            text(
                p,
                crate::i18n::msg::stash_player_o_question(),
                Rect::new(670., 470., 800., 90.),
                theme::size::HEADLINE,
            );
            text(
                p,
                &crate::i18n::msg::stash_player_count(view.o_count),
                Rect::new(670., 560., 800., 40.),
                theme::size::CAPTION,
            );
        } else {
            let hud_up = self.visible(f.cx);
            player_hud::draw_subtitles(hud_up, false);
            player_hud::draw_subtitle_bitmap(&mut self.subtitles, hud_up);
            if !self.visible(f.cx) {
                return;
            }
            let hud = p.translate(0., -self.offset.pos);
            player_hud::draw_scrim(p);
            if self.offset.pos > 0. {
                player_hud::draw_scrim(hud);
            }
            let title = view.scene.as_ref().map(|s| s.display_title()).unwrap_or("");
            let title = CString::new(title.replace('\0', "")).unwrap_or_default();
            let count = CString::new(crate::i18n::msg::stash_player_count(view.o_count))
                .unwrap_or_default();
            player_hud::draw_title_fitted(
                hud,
                player_hud::Kicker::Context(count.as_ptr()),
                title.as_ptr(),
                player_hud::disc_hit_rect(0).x - 32.,
                f.cx.measure,
            );
            player_hud::draw_playbar(
                hud,
                Playbar {
                    pos_ns: (self.state.scrub.unwrap_or(view.position) * 1e9) as i64,
                    dur_ns: (view.duration * 1e9) as i64,
                    knob: if f.cx.focus.current.is_some_and(|k| k.elem == 0) {
                        Knob::Focused
                    } else {
                        Knob::Tick
                    },
                    mark: if view.loading {
                        TransportMark::Working
                    } else if !view.playing {
                        TransportMark::Pause
                    } else {
                        TransportMark::None
                    },
                    now: f.cx.tick.ms,
                },
                f.cx.measure,
            );
        }
        if self.state.expanded && !view.completed {
            self.markers.draw(
                p,
                f.cx.views.textures,
                f.cx.focus.current.map(|k| k.elem),
                self.offset.pos,
                self.markers
                    .preview(
                        f.cx.focus
                            .current
                            .map(|k| k.elem)
                            .filter(|k| Some(*k) != self.state.suppressed_preview),
                    )
                    .and_then(|(key, _)| {
                        f.cx.views.textures.get(&format!("preview:{key}")).copied()
                    }),
                f.cx.measure,
            );
        }
        for key in self.keys(f.cx) {
            let Some(rect) = self.rect(key, f.cx) else {
                continue;
            };
            f.stop(
                p,
                Stop {
                    key: FocusKey {
                        entry: self.entry,
                        elem: key,
                    },
                    rect,
                    rest_rect: rect,
                    clip: Rect::FULL,
                    hover: Hover::Focus,
                    activate: if key == 0 {
                        Activate::Immediate
                    } else {
                        Activate::Press
                    },
                },
            );
            if key == 0 || self.markers.contains(key) {
                continue;
            }
            let focused =
                f.cx.focus
                    .current
                    .is_some_and(|k| k.elem == key && k.entry == self.entry);
            let label = match key {
                3 | 11 => "+".to_owned(),
                10 => crate::i18n::msg::stash_player_replay().to_owned(),
                _ => continue,
            };
            let label = CString::new(label.replace('\0', "")).unwrap_or_default();
            let button = Button::new(label.as_ptr(), theme::size::BODY, rect)
                .focused(focused)
                .ground(ControlGround::Unkeyed);
            let button = if key == 3 || key == 11 {
                button.icon(crate::ui::icons::Icon::Droplets)
            } else {
                button
            };
            let cooling = (key == 3 || key == 11)
                && (view.o_pending
                    || self
                        .state
                        .o_last
                        .is_some_and(|last| f.cx.tick.ms.wrapping_sub(last) < 2000));
            button.draw(
                &Env::inert(),
                if cooling {
                    p.alpha(theme::INK_DISABLED[3])
                } else {
                    p
                },
            );
        }
        if !view.error.is_empty() {
            text(
                p,
                &view.error,
                Rect::new(72., 120., 1600., 64.),
                theme::size::BODY,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
    fn context(view: &PlaybackView, test: impl FnOnce(&Cx<'_, StashHost>)) {
        let config = crate::stash::Config::default();
        let textures = std::collections::HashMap::new();
        test(&Cx {
            views: Views {
                textures: &textures,
                config: &config,
                playback: view,
            },
            tick: Tick::default(),
            measure: &Measure,
            press: PressRead::default(),
            focus: FocusRead::default(),
            owner: InputOwner::Entry(EntryId(1)),
        });
    }
    #[test]
    fn committed_keyboard_press_reaches_timeline_and_marker_actions() {
        let mut scene = crate::stash::Scene::default();
        scene.scene_markers.push(crate::stash::SceneMarker {
            id: "press-marker".into(),
            seconds: 42.,
            ..Default::default()
        });
        context(
            &PlaybackView {
                scene: Some(scene.clone()),
                playing: true,
                ..Default::default()
            },
            |cx| {
                let mut screen = PlayerScreen::new("1".into(), EntryId(1));
                screen.markers.sync(&scene.scene_markers);
                screen.state.expanded = true;
                let marker = screen.markers.keys()[0];
                for (key, seek) in [(0, false), (marker, true)] {
                    let mut cx = Cx {
                        views: cx.views,
                        tick: cx.tick,
                        measure: cx.measure,
                        press: cx.press,
                        focus: cx.focus.clone(),
                        owner: cx.owner,
                    };
                    cx.focus.current = Some(FocusKey {
                        entry: EntryId(1),
                        elem: key,
                    });
                    let mut out = Vec::new();
                    let mut present = crate::ui::present::Present::new();
                    let mut fx =
                        Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                    screen.step(&ScreenEvent::PressCommit(PressId(1)), &cx, &mut fx);
                    assert_eq!(
                        out.iter()
                            .filter(|event| matches!(&event.fx,
                    Fx::App(StashFx::Player(Action::Pause)) if !seek)
                                || matches!(&event.fx,
                    Fx::App(StashFx::Player(Action::SeekTo(42.))) if seek))
                            .count(),
                        1,
                        "committed keyboard control must reach playback"
                    );
                }
            },
        );
    }
    #[test]
    fn original_disc_and_scrubber_geometry_is_preserved() {
        assert!(player_hud::scrub_hit_rect().w > 1000.);
        assert!(player_hud::disc_hit_rect(0).x < player_hud::disc_hit_rect(1).x);
        assert_eq!(player_hud::disc_hit_rect(0).h, 64.);
    }
    #[test]
    fn original_up_ring_reaches_controls_then_hides() {
        context(&PlaybackView::default(), |cx| {
            let mut screen = PlayerScreen::new("1".into(), EntryId(1));
            let focus = FocusKey {
                entry: EntryId(1),
                elem: 0,
            };
            assert!(matches!(
                screen.neighbour(focus, Dir::Up, cx),
                Step::Move(FocusKey { elem: 3, .. })
            ));
            let mut cx = Cx {
                views: cx.views,
                tick: cx.tick,
                measure: cx.measure,
                press: cx.press,
                focus: cx.focus.clone(),
                owner: cx.owner,
            };
            cx.focus.current = Some(FocusKey {
                entry: EntryId(1),
                elem: 3,
            });
            let mut out = Vec::new();
            let mut present = crate::ui::present::Present::new();
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            let event = ScreenEvent::Input(InputEvent {
                at: Tick::default(),
                source: Source::Script,
                kind: InputKind::Key {
                    key: Key::Up,
                    sym: 0,
                    wcode: 0,
                    edge: Edge::Down,
                    at_edge: true,
                },
            });
            assert!(matches!(screen.step(&event, &cx, &mut fx), Handled::Yes));
            assert!(screen.state.dismissed);
        });
    }
    #[test]
    fn marker_activation_only_emits_seek_and_preserves_pause_state() {
        let mut scene = crate::stash::Scene::default();
        scene.scene_markers.push(crate::stash::SceneMarker {
            id: "m".into(),
            title: "Chapter".into(),
            seconds: 29.4,
            ..Default::default()
        });
        context(
            &PlaybackView {
                scene: Some(scene.clone()),
                playing: false,
                ..Default::default()
            },
            |cx| {
                let mut screen = PlayerScreen::new("1".into(), EntryId(1));
                screen.markers.sync(&scene.scene_markers);
                screen.state.expanded = true;
                let key = screen.markers.keys()[0];
                assert!(
                    matches!(screen.neighbour(FocusKey { entry: EntryId(1), elem: 0 }, Dir::Down, cx), Step::Move(FocusKey { elem, .. }) if elem == key)
                );
                assert!(matches!(
                    screen.neighbour(
                        FocusKey {
                            entry: EntryId(1),
                            elem: key
                        },
                        Dir::Up,
                        cx
                    ),
                    Step::Move(FocusKey { elem: 3, .. })
                ));
                let mut out = Vec::new();
                let mut present = crate::ui::present::Present::new();
                let mut fx =
                    Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                screen.activate(key, cx, &mut fx);
                assert_eq!(
                    out.iter()
                        .filter(|s| matches!(&s.fx,
                    Fx::App(StashFx::Player(Action::SeekTo(p))) if *p == 29.4))
                        .count(),
                    1
                );
                assert_eq!(screen.state.suppressed_preview, Some(key));
                assert!(!cx.views.playback.playing);
            },
        );
    }
    #[test]
    fn counter_cooldown_applies_to_live_and_completed_controls() {
        for (key, completed) in [(3, false), (11, true)] {
            context(
                &PlaybackView {
                    completed,
                    ..Default::default()
                },
                |cx| {
                    let mut screen = PlayerScreen::new("1".into(), EntryId(1));
                    let mut out = Vec::new();
                    let mut present = crate::ui::present::Present::new();
                    for ms in [1000, 1001, 2999, 3000] {
                        let mut cx = Cx {
                            views: cx.views,
                            tick: cx.tick,
                            measure: cx.measure,
                            press: cx.press,
                            focus: cx.focus.clone(),
                            owner: cx.owner,
                        };
                        cx.tick.ms = ms;
                        let mut fx = Effects::new(
                            &mut out,
                            MachineId::Instance(InstanceId(1)),
                            &mut present,
                        );
                        screen.activate(key, &cx, &mut fx);
                    }
                    assert_eq!(
                        out.iter()
                            .filter(|s| matches!(&s.fx, Fx::App(StashFx::Player(Action::AddO))))
                            .count(),
                        2
                    );
                },
            );
        }
    }
    #[test]
    fn bitmap_subtitle_gpu_storage_is_reported() {
        let mut screen = PlayerScreen::new("1".into(), EntryId(1));
        assert_eq!(
            Screen::<StashHost>::render_report(&screen),
            crate::ui::frame::RenderReport::NONE
        );
        screen.subtitles = player_hud::SubtitleBitmaps::stub(&[(720, 120)]);
        assert_eq!(
            Screen::<StashHost>::render_report(&screen),
            crate::ui::frame::RenderReport::one(720, 120)
        );
    }
    #[test]
    fn live_hud_has_only_scrub_and_counter_without_redundant_pause() {
        context(&PlaybackView::default(), |cx| {
            let screen = PlayerScreen::new("1".into(), EntryId(1));
            assert_eq!(screen.keys(cx), [0, 3]);
            for (key, expected) in [(0, player_hud::scrub_hit_rect())] {
                let actual = screen.rect(key, cx).unwrap();
                assert_eq!(
                    [actual.x, actual.y, actual.w, actual.h],
                    [expected.x, expected.y, expected.w, expected.h]
                );
            }
            assert!(screen.rect(10, cx).is_none());
            assert!(screen.rect(1, cx).is_none());
            assert!(screen.rect(2, cx).is_none());
        });
    }
    #[test]
    fn completion_replaces_hud_and_repairs_focus_to_replay() {
        context(
            &PlaybackView {
                completed: true,
                ..Default::default()
            },
            |cx| {
                let screen = PlayerScreen::new("1".into(), EntryId(1));
                assert_eq!(screen.keys(cx), [10, 11]);
                assert!(screen.rect(0, cx).is_none());
                assert_eq!(
                    screen
                        .reconcile(
                            FocusKey {
                                entry: EntryId(1),
                                elem: 2
                            },
                            cx
                        )
                        .elem,
                    10
                );
            },
        );
    }
    #[test]
    fn completed_counter_refuses_overlap_and_held_ok_does_not_activate() {
        let _lock = crate::testlock::serial();
        context(
            &PlaybackView {
                completed: true,
                o_pending: true,
                ..Default::default()
            },
            |cx| {
                let mut screen = PlayerScreen::new("1".into(), EntryId(1));
                let mut out = Vec::new();
                let mut present = crate::ui::present::Present::new();
                let mut fx =
                    Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                screen.activate(11, cx, &mut fx);
                let event = ScreenEvent::Input(InputEvent {
                    at: Tick::default(),
                    source: Source::Script,
                    kind: InputKind::Key {
                        key: Key::Ok,
                        sym: 0,
                        wcode: 0,
                        edge: Edge::Repeat,
                        at_edge: false,
                    },
                });
                assert!(matches!(screen.step(&event, cx, &mut fx), Handled::Yes));
                assert!(out.is_empty());
            },
        );
    }
}
