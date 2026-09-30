//! Stash transport composed from the original shared video-plane HUD.
use crate::screens::stash_registry::*;
use crate::ui::frame::Budget;
use crate::ui::machine::*;
use crate::ui::player_hud::{self, ControlSlot, Knob, Playbar, TransportMark, TransportRow};
use crate::ui::screen::*;
use crate::ui::widgets::{Button, ControlGround, TransportButton};
use crate::ui::{theme, Env, Rect, View};
use std::{borrow::Cow, ffi::CString};

const LINGER: u32 = 4500;
#[derive(Default)]
struct State {
    until: u32,
    dismissed: bool,
    panel: u8,
    scrub: Option<f64>,
    completed: bool,
}
impl LogicalState for State {
    fn write(&self, c: &mut Canon) {
        c.u32(self.until)
            .bool(self.dismissed)
            .u32(self.panel as u32)
            .bool(self.completed)
            .f32(self.scrub.unwrap_or(-1.) as f32);
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!(
            "completed={} panel={}",
            self.completed, self.panel
        ));
    }
}
pub struct PlayerScreen {
    id: String,
    entry: EntryId,
    state: State,
    row: TransportRow,
    subtitles: player_hud::SubtitleBitmaps,
}
impl PlayerScreen {
    pub fn new(id: String, entry: EntryId) -> Self {
        Self {
            id,
            entry,
            state: State::default(),
            row: TransportRow::new(),
            subtitles: player_hud::SubtitleBitmaps::new(),
        }
    }
    fn visible(&self, cx: &Cx<'_, StashHost>) -> bool {
        cx.views.playback.completed
            || self.state.panel != 0
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
        } else if self.state.panel != 0 {
            let n = if self.state.panel == 1 {
                cx.views.playback.audio.len()
            } else {
                cx.views.playback.subtitles.len() + 1
            };
            (100..100 + n as u32).collect()
        } else if self.visible(cx) {
            vec![0, 1, 2, 3, 4]
        } else {
            Vec::new()
        }
    }
    fn rect(&self, key: u32, cx: &Cx<'_, StashHost>) -> Option<Rect> {
        if !self.keys(cx).contains(&key) {
            return None;
        }
        Some(match key {
            0 => player_hud::scrub_hit_rect(),
            1 | 2 => player_hud::disc_hit_rect((key - 1) as i32),
            3 => {
                let r = player_hud::disc_hit_rect(2);
                Rect::new(r.x - 24., r.y, 112., r.h)
            }
            4 => Rect::new(72., 962., 240., 64.),
            10 => Rect::new(670., 620., 260., 64.),
            11 => Rect::new(970., 620., 260., 64.),
            _ => {
                let scroll = cx
                    .focus
                    .current
                    .map(|k| k.elem)
                    .unwrap_or(100)
                    .saturating_sub(106);
                Rect::new(
                    1170.,
                    170. + (key as i64 - 100 - scroll as i64) as f32 * 68.,
                    620.,
                    60.,
                )
            }
        })
    }
    fn emit(&self, action: Action, fx: &mut Effects<'_, StashHost>) {
        fx.push(Fx::App(StashFx::Player(action)));
    }
    fn activate(&mut self, key: u32, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        self.reveal(cx.tick.ms);
        match key {
            1 => {
                self.state.panel = 2;
            }
            2 => {
                self.state.panel = 1;
            }
            3 | 11 if !cx.views.playback.o_pending => self.emit(Action::AddO, fx),
            4 => self.emit(Action::Pause, fx),
            10 => {
                self.state.completed = false;
                self.emit(Action::Replay, fx);
            }
            n if n >= 100 => {
                self.emit(
                    if self.state.panel == 1 {
                        Action::AudioTrack((n - 100) as i32)
                    } else {
                        Action::SubtitleTrack((n - 100) as i32 - 1)
                    },
                    fx,
                );
                self.state.panel = 0;
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
        if k.elem == 0 && matches!(d, Dir::Left | Dir::Right) {
            return Step::Edge;
        }
        let next = match d {
            Dir::Left | Dir::Up => i.checked_sub(1),
            Dir::Right | Dir::Down => (i + 1 < keys.len()).then_some(i + 1),
        };
        next.map(|i| {
            Step::Move(FocusKey {
                entry: self.entry,
                elem: keys[i],
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
            ScreenEvent::Unmount | ScreenEvent::Suspend => {
                self.subtitles.release();
                Handled::Yes
            }
            ScreenEvent::Mount | ScreenEvent::Uncover => {
                self.reveal(cx.tick.ms);
                Handled::Yes
            }
            ScreenEvent::Tick(t) => {
                if cx.views.playback.completed && !self.state.completed {
                    self.state.completed = true;
                    self.state.panel = 0;
                    self.state.scrub = None;
                }
                let focus = cx.focus.current.map(|k| k.elem).unwrap_or(0);
                self.row.step(
                    ControlSlot::Discs,
                    if focus == 0 { 0 } else { 1 },
                    focus.saturating_sub(1) as i32,
                    t.dt(),
                    t.ms,
                );
                Handled::Yes
            }
            ScreenEvent::Activate(k) => {
                self.activate(*k, cx, fx);
                Handled::Yes
            }
            ScreenEvent::FocusMoved { .. } => {
                self.reveal(cx.tick.ms);
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
                if self.state.panel != 0 {
                    self.state.panel = 0;
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
                    Handled::Yes
                } else if !cx.views.playback.completed
                    && self.state.panel == 0
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
                    TransportKey::Play
                        if !cx.views.playback.playing && !cx.views.playback.completed =>
                    {
                        self.emit(Action::Pause, fx);
                        self.reveal(cx.tick.ms);
                        Handled::Yes
                    }
                    TransportKey::Pause if cx.views.playback.playing => {
                        self.emit(Action::Pause, fx);
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
            player_hud::draw_scrim(p);
            let title = view.scene.as_ref().map(|s| s.display_title()).unwrap_or("");
            let title = CString::new(title.replace('\0', "")).unwrap_or_default();
            let count = CString::new(crate::i18n::msg::stash_player_count(view.o_count))
                .unwrap_or_default();
            player_hud::draw_title(
                p,
                player_hud::Kicker::Context(count.as_ptr()),
                title.as_ptr(),
            );
            player_hud::draw_playbar(
                p,
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
        if self.state.panel != 0 {
            text(
                p,
                if self.state.panel == 1 {
                    crate::i18n::msg::stash_player_audio()
                } else {
                    crate::i18n::msg::stash_player_subtitles()
                },
                Rect::new(1170., 90., 620., 64.),
                theme::size::TITLE,
            );
        }
        for key in self.keys(f.cx) {
            let Some(rect) = self.rect(key, f.cx) else {
                continue;
            };
            if self.state.panel != 0 && (rect.y < 170. || rect.y + rect.h > 918.) {
                continue;
            }
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
            if key == 0 {
                continue;
            }
            let focused =
                f.cx.focus
                    .current
                    .is_some_and(|k| k.elem == key && k.entry == self.entry);
            if key == 1 || key == 2 {
                TransportButton::new((key - 1) as i32, rect)
                    .focused(focused)
                    .ground(ControlGround::Unkeyed)
                    .scale(self.row.scale((key - 1) as i32))
                    .draw(&Env::inert(), p);
                continue;
            }
            let label = match key {
                3 | 11 => crate::i18n::msg::stash_player_o_plus().to_owned(),
                4 => crate::i18n::msg::stash_player_play_pause().to_owned(),
                10 => crate::i18n::msg::stash_player_replay().to_owned(),
                n if self.state.panel == 1 => view
                    .audio
                    .get((n - 100) as usize)
                    .cloned()
                    .unwrap_or_default(),
                100 => crate::i18n::msg::stash_player_subtitles_off().to_owned(),
                n => view
                    .subtitles
                    .get((n - 101) as usize)
                    .cloned()
                    .unwrap_or_default(),
            };
            let selected = key >= 100
                && if self.state.panel == 1 {
                    key as i32 - 100 == view.selected_audio
                } else {
                    key as i32 - 101 == view.selected_subtitle
                };
            let label = if selected {
                format!("✓ {label}")
            } else {
                label
            };
            let label = CString::new(label.replace('\0', "")).unwrap_or_default();
            Button::new(label.as_ptr(), theme::size::BODY, rect)
                .focused(focused)
                .ground(ControlGround::Unkeyed)
                .draw(&Env::inert(), p);
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
    fn original_disc_and_scrubber_geometry_is_preserved() {
        assert!(player_hud::scrub_hit_rect().w > 1000.);
        assert!(player_hud::disc_hit_rect(0).x < player_hud::disc_hit_rect(1).x);
        assert_eq!(player_hud::disc_hit_rect(0).h, 64.);
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
    fn live_hud_has_scrub_tracks_counter_and_transport_only() {
        context(&PlaybackView::default(), |cx| {
            let screen = PlayerScreen::new("1".into(), EntryId(1));
            assert_eq!(screen.keys(cx), [0, 1, 2, 3, 4]);
            for (key, expected) in [
                (0, player_hud::scrub_hit_rect()),
                (1, player_hud::disc_hit_rect(0)),
                (2, player_hud::disc_hit_rect(1)),
            ] {
                let actual = screen.rect(key, cx).unwrap();
                assert_eq!(
                    [actual.x, actual.y, actual.w, actual.h],
                    [expected.x, expected.y, expected.w, expected.h]
                );
            }
            assert!(screen.rect(10, cx).is_none());
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
