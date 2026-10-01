//! Stash scene metadata in the shared movie-detail hero and shelf layout.
use super::catalog::StashScreen;
use crate::screens::stash_registry::*;
use crate::stash::Scene;
use crate::ui::consts::{MARGIN_X, SCR_W};
use crate::ui::detail_layout::{self, HERO_TEXT_W, TITLE_BOTTOM};
use crate::ui::frame::Budget;
use crate::ui::label::{Label, VAlign};
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::text_view::TextView;
use crate::ui::widgets::{Button, CircleButton, StatusOverlay};
use crate::ui::{theme, Env, Painter, Rect, View};
use std::{borrow::Cow, ffi::CString};

const PLAY: u32 = 0x1000_0000;
const RESTART: u32 = PLAY + 1;
const HERO: GroupId = GroupId(0);

pub struct SceneDetailScreen {
    entry: EntryId,
    content: StashScreen,
}
impl SceneDetailScreen {
    pub fn new(id: String, entry: EntryId) -> Self {
        Self {
            entry,
            content: StashScreen::detail(entry, id),
        }
    }
    fn synopsis<'a>(&'a self, cx: &'a Cx<'_, StashHost>) -> TextView<'a> {
        TextView::new(
            self.content
                .scene()
                .and_then(|s| s.details.as_deref())
                .unwrap_or(""),
            theme::size::BODY,
            theme::TEXT_SECONDARY,
        )
        .with_measure(cx.measure)
        .max_lines(3)
    }
    fn chain(&self, cx: &Cx<'_, StashHost>) -> detail_layout::HeroChain {
        detail_layout::hero_chain(self.synopsis(cx).measure_h(HERO_TEXT_W), false, cx.measure)
    }
    fn resume(&self) -> bool {
        self.content.scene().is_some_and(|s| s.resume_time > 0.)
    }
    fn keys(&self) -> &[u32] {
        if self.content.scene().is_none() {
            &[]
        } else if self.resume() {
            &[PLAY, RESTART]
        } else {
            &[PLAY]
        }
    }
    fn play_label(&self) -> &'static std::ffi::CStr {
        if self.content.loading() {
            crate::i18n::msg::browse_library_loading_c()
        } else if self.resume() {
            crate::i18n::msg::browse_detail_resume_c()
        } else {
            crate::i18n::msg::browse_detail_play_c()
        }
    }
    fn rect(&self, key: u32, cx: &Cx<'_, StashHost>) -> Option<Rect> {
        if !self.keys().contains(&key) {
            return None;
        }
        let width = Button::pill_w(self.play_label().as_ptr(), theme::size::BODY, true).max(168.);
        let y = self.chain(cx).btn_y - self.content.scroll();
        Some(if key == PLAY {
            Rect::new(MARGIN_X, y, width, StatusOverlay::CTRL_H)
        } else {
            Rect::new(
                MARGIN_X + width + crate::ui::widgets::CTRL_GAP,
                y,
                StatusOverlay::CTRL_H,
                StatusOverlay::CTRL_H,
            )
        })
    }
    fn activate(&self, key: u32, fx: &mut Effects<'_, StashHost>) -> bool {
        if !self.keys().contains(&key) {
            return false;
        }
        if self.content.loading() {
            return true;
        }
        if let Some(scene) = self.content.scene() {
            fx.push(Fx::App(StashFx::Play(
                scene.clone(),
                key == PLAY && self.resume(),
            )));
            fx.push(Fx::Nav(NavOp::Push(StashArg::Player(scene.id.clone()))));
        }
        true
    }
    fn draw_hero(&self, f: &mut DrawFrame<'_, '_, StashHost>, scene: &Scene) {
        let scroll = self.content.scroll();
        let visible = (1. - scroll / TITLE_BOTTOM).clamp(0., 1.);
        let p = f.painter;
        let fill = theme::SURFACE_APP;
        p.rect(Rect::FULL, 0., fill, fill, 0.);
        if let Some(&(texture, w, h)) =
            f.cx.views
                .textures
                .get(&format!("preview:scene:{}", scene.id))
                .or_else(|| f.cx.views.textures.get(&format!("scene:{}", scene.id)))
        {
            p.tex_uv(
                texture,
                Rect::FULL.cover_uv(w, h, crate::ui::Crop::Centre),
                Rect::FULL,
                0.,
                theme::with_a(theme::TEXT_PRIMARY, visible),
            );
        }
        crate::ui::widgets::hero_scrim(p, visible, !scene.performers.is_empty());
        let y0 = crate::ui::widgets::HERO_BASE_SCRIM_Y0;
        p.rect(
            Rect::new(0., y0, SCR_W, crate::ui::consts::SCR_H - y0),
            0.,
            theme::scrim(0.),
            theme::scrim(detail_layout::base_scrim_a(
                crate::ui::consts::SCR_H,
                visible,
            )),
            0.,
        );
        let p = p.alpha(visible);
        let title = crate::text::elide_by(scene.display_title(), HERO_TEXT_W, false, |s| {
            f.cx.measure.width_str(s, theme::size::HERO, true)
        });
        let title = cs(&title);
        Label::new(title.as_ptr(), theme::size::HERO, theme::TEXT_PRIMARY)
            .bold()
            .v(VAlign::Baseline)
            .draw(
                p,
                Rect::new(
                    MARGIN_X,
                    TITLE_BOTTOM
                        - crate::ui::hero_logo::band_h(crate::ui::hero_logo::LogoRung::Hero)
                        - scroll,
                    HERO_TEXT_W,
                    crate::ui::hero_logo::band_h(crate::ui::hero_logo::LogoRung::Hero),
                ),
            );
        let chain = self.chain(f.cx);
        let metadata = scene.studio.as_ref().map(|s| s.name.as_str()).unwrap_or("");
        line(
            p,
            metadata,
            Rect::new(
                MARGIN_X,
                chain.meta_y - scroll,
                HERO_TEXT_W,
                crate::ui::widgets::BADGE_H,
            ),
            theme::size::BODY,
            theme::TEXT_SECONDARY,
        );
        self.synopsis(f.cx).draw(
            p,
            Rect::new(MARGIN_X, chain.syn_y - scroll, HERO_TEXT_W, 0.),
        );
        let file = scene.files.first();
        let duration = file.map(|s| s.duration).unwrap_or(0.);
        let facts = format!(
            "{} · {}:{:02} · {} {}{}",
            scene.date.as_deref().unwrap_or(""),
            (duration / 60.) as u32,
            (duration % 60.) as u32,
            crate::i18n::msg::stash_detail_o_count(),
            scene.o_counter,
            file.map(|s| format!(
                " · {}p · {} / {}",
                s.height,
                s.video_codec.to_uppercase(),
                s.audio_codec.to_uppercase()
            ))
            .unwrap_or_default()
        );
        line(
            p,
            &facts,
            Rect::new(
                MARGIN_X,
                chain.facts_y - scroll,
                HERO_TEXT_W,
                f.cx.measure.cap_h(theme::size::CAPTION),
            ),
            theme::size::CAPTION,
            detail_layout::FACTS_INK,
        );
        let names = scene
            .performers
            .iter()
            .take(detail_layout::PEOPLE_MAX_LINES)
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>();
        let top = detail_layout::people_top(chain.btn_y, names.len()) - scroll;
        for (index, name) in names.iter().enumerate() {
            line(
                p,
                name,
                Rect::new(
                    SCR_W - MARGIN_X - detail_layout::PEOPLE_W,
                    top + index as f32 * detail_layout::PEOPLE_LEAD,
                    detail_layout::PEOPLE_W,
                    detail_layout::PEOPLE_LEAD,
                ),
                theme::size::CAPTION,
                detail_layout::PEOPLE_INK,
            );
        }
        for &key in self.keys() {
            let Some(rect) = self.rect(key, f.cx) else {
                continue;
            };
            if rect.y + rect.h < detail_layout::TOP_MARGIN {
                continue;
            }
            let focused =
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
                    rest_rect: rect,
                    clip: Rect::FULL,
                    hover: Hover::Focus,
                    activate: Activate::Press,
                },
            );
            if key == PLAY {
                Button::new(self.play_label().as_ptr(), theme::size::BODY, rect)
                    .icon(crate::ui::icons::Icon::Play)
                    .focused(focused)
                    .draw(&Env::inert(), p);
            } else {
                CircleButton::new(c"".as_ptr())
                    .frame(rect)
                    .icon(crate::ui::icons::Icon::Restart)
                    .focused(focused)
                    .draw(&Env::inert(), p);
            }
        }
    }
}
fn cs(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap()
}
fn line(p: Painter, text: &str, rect: Rect, size: i32, ink: [f32; 4]) {
    let text = cs(text);
    Label::new(text.as_ptr(), size, ink)
        .v(VAlign::CapTop)
        .draw(p, rect);
}
impl Focusable<StashHost> for SceneDetailScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        if let Some(rect) = self.rect(PLAY, cx) {
            out.push(GroupSpec {
                id: HERO,
                kind: GroupKind::Row { wrap: false },
                seat: Seat::First,
                reachable: AxisMask::BOTH,
                edge: [
                    EdgeRule::Stop,
                    EdgeRule::Geometric,
                    EdgeRule::Stop,
                    EdgeRule::Stop,
                ],
                extent: rect,
                len: self.keys().len(),
                elem: ElemKind::Control,
            });
        }
        self.content.groups(cx, out);
    }
    fn group_of(&self, key: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        if self.keys().contains(key) {
            Some(HERO)
        } else {
            self.content.group_of(key, cx)
        }
    }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        if self.keys().contains(&key.elem) {
            let next = match (key.elem, dir) {
                (PLAY, Dir::Right) if self.resume() => Some(RESTART),
                (RESTART, Dir::Left) => Some(PLAY),
                _ => None,
            };
            next.map(|elem| {
                Step::Move(FocusKey {
                    entry: self.entry,
                    elem,
                })
            })
            .unwrap_or(Step::Edge)
        } else {
            self.content.neighbour(key, dir, cx)
        }
    }
    fn place(&self, key: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        self.rect(*key, cx)
            .map(|rect| Placed {
                rect,
                rest_rect: rect,
                clip: Rect::FULL,
                index: Some(key - PLAY),
            })
            .or_else(|| self.content.place(key, cx, at))
    }
    fn reconcile(&self, key: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if self.keys().contains(&key.elem) {
            key
        } else if self.content.group_of(&key.elem, cx).is_some() {
            self.content.reconcile(key, cx)
        } else if self.content.scene().is_some() {
            FocusKey {
                entry: self.entry,
                elem: PLAY,
            }
        } else {
            self.content.reconcile(key, cx)
        }
    }
    fn seat(&self, group: GroupId, from: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if group == HERO {
            FocusKey {
                entry: self.entry,
                elem: PLAY,
            }
        } else {
            self.content.seat(group, from, cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::stash::PageData;
    fn context(test: impl FnOnce(&Cx<'_, StashHost>)) {
        let textures = Default::default();
        let config = Default::default();
        let playback = Default::default();
        test(&Cx {
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
        });
    }
    #[test]
    fn arriving_metadata_seats_play_and_resume_restart_keep_scene_identity() {
        let _lock = crate::testlock::serial();
        context(|cx| {
            let mut screen = SceneDetailScreen::new("42".into(), EntryId(1));
            let mut out = Vec::new();
            let mut present = crate::ui::present::Present::new();
            {
                let mut fx =
                    Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                screen.step(&ScreenEvent::Mount, cx, &mut fx);
                screen.step(
                    &ScreenEvent::Async(
                        RequestId(1),
                        StashMsg::Loaded {
                            generation: 1,
                            result: Ok(PageData {
                                scene: Some(Scene {
                                    id: "42".into(),
                                    resume_time: 120.,
                                    details: Some("Scene synopsis".into()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                        },
                    ),
                    cx,
                    &mut fx,
                );
            }
            assert_eq!(
                screen
                    .reconcile(
                        FocusKey {
                            entry: EntryId(1),
                            elem: 0
                        },
                        cx
                    )
                    .elem,
                PLAY
            );
            out.clear();
            {
                let mut fx =
                    Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
                screen.activate(PLAY, &mut fx);
                screen.activate(RESTART, &mut fx);
            }
            let plays = out
                .iter()
                .filter_map(|effect| match &effect.fx {
                    Fx::App(StashFx::Play(scene, resume)) => Some((scene.id.as_str(), *resume)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(plays, [("42", true), ("42", false)]);
            assert_eq!(
                out.iter()
                    .filter(
                        |e| matches!(&e.fx,Fx::Nav(NavOp::Push(StashArg::Player(id))) if id=="42")
                    )
                    .count(),
                2
            );
        });
    }
}
impl Machine<StashHost> for SceneDetailScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        event: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        self.content
            .detail_inset(self.chain(cx).btn_y + StatusOverlay::CTRL_H + theme::space::XL);
        let preview = if self.content.scroll() < TITLE_BOTTOM {
            self.content.scene().and_then(|s| {
                s.paths
                    .preview
                    .as_ref()
                    .map(|url| (format!("scene:{}", s.id), url.clone()))
            })
        } else {
            None
        };
        self.content.media_override(Vec::new(), preview);
        match event {
            ScreenEvent::Activate(key) if self.activate(*key, fx) => Handled::Yes,
            ScreenEvent::PressCommit(_)
                if cx.focus.current.is_some_and(|k| self.activate(k.elem, fx)) =>
            {
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } if self.keys().contains(&to.elem) => {
                self.content.shelf_scroll(0.);
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            _ => self.content.step(event, cx, fx),
        }
    }
}
impl Screen<StashHost> for SceneDetailScreen {
    fn name(&self) -> &'static str {
        "scene"
    }
    fn state(&self) -> &dyn LogicalState {
        self.content.state()
    }
    fn crumb(&self, cx: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        self.content.crumb(cx)
    }
    fn prepare(&mut self, b: &mut Budget, cx: &Cx<'_, StashHost>) {
        self.content.prepare(b, cx);
    }
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn links(&self, out: &mut Vec<Link>) {
        if let Some(group) = self.content.first_group() {
            out.push(Link {
                from: HERO,
                dir: Dir::Down,
                to: group,
            });
            out.push(Link {
                from: group,
                dir: Dir::Up,
                to: HERO,
            });
        }
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        if let Some(scene) = self.content.scene() {
            self.draw_hero(f, scene);
        }
        self.content.draw(f);
    }
}
