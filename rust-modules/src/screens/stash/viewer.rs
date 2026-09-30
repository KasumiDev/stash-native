//! Gallery viewer owns image selection, loading, and slideshow timing.
use crate::screens::stash_registry::*;
use crate::stash::{Image, Query};
use crate::stores::stash::Work;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::{theme, Env, Rect, View};
use std::borrow::Cow;
pub struct ViewerScreen {
    entry: EntryId,
    gallery: String,
    index: usize,
    images: Vec<Image>,
    generation: u32,
    loading: bool,
    error: String,
    slideshow: bool,
    interval: u32,
    last_slide: u32,
    covered: bool,
}
impl ViewerScreen {
    pub fn new(entry: EntryId, gallery: String, index: usize) -> Self {
        Self {
            entry,
            gallery,
            index,
            images: Vec::new(),
            generation: 0,
            loading: false,
            error: String::new(),
            slideshow: false,
            interval: 5,
            last_slide: 0,
            covered: false,
        }
    }
    fn rects(&self) -> [Rect; 4] {
        [
            Rect::new(96., 944., 300., 64.),
            Rect::new(416., 944., 300., 64.),
            Rect::new(736., 944., 540., 64.),
            Rect::new(1296., 944., 300., 64.),
        ]
    }
    fn controls<R>(&self, f: impl FnOnce(crate::ui::geom::TabRow<'_>) -> R) -> R {
        let rects = self.rects();
        f(crate::ui::geom::TabRow {
            rects: &rects,
            group: GroupId(0),
            entry: self.entry,
        })
    }
    fn load(&mut self, fx: &mut Effects<'_, StashHost>) {
        self.generation += 1;
        self.loading = true;
        self.error.clear();
        fx.push(Fx::App(StashFx::Work(
            Addr {
                to: fx.from(),
                req: RequestId(self.generation),
            },
            Work::Load {
                route: StashArg::Viewer {
                    gallery: self.gallery.clone(),
                    index: self.index,
                },
                query: Query::default(),
                generation: self.generation,
            },
        )));
    }
    fn media(&self, fx: &mut Effects<'_, StashHost>) {
        let n = self.images.len();
        let mut images = Vec::new();
        if n > 0 {
            for i in [self.index, (self.index + 1) % n, (self.index + n - 1) % n] {
                if let Some(image) = self.images.get(i) {
                    if let Some(url) = &image.paths.image {
                        let key = format!("image:{}", image.id);
                        if !images.iter().any(|(k, _)| k == &key) {
                            images.push((key, url.clone()));
                        }
                    }
                }
            }
        }
        fx.push(Fx::App(StashFx::Media(images, None)));
    }
    fn advance(&mut self, delta: i32, now: u32, fx: &mut Effects<'_, StashHost>) {
        if !self.images.is_empty() {
            self.index = (self.index as i32 + delta).rem_euclid(self.images.len() as i32) as usize;
            self.last_slide = now;
            self.media(fx);
            fx.invalidate(crate::ui::present::Provenance::Input);
        }
    }
    fn activate(&mut self, key: u32, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) {
        if !self.error.is_empty() {
            self.load(fx);
            return;
        }
        match key {
            0 => self.advance(-1, cx.tick.ms, fx),
            1 => self.advance(1, cx.tick.ms, fx),
            2 => {
                self.slideshow = !self.slideshow;
                self.last_slide = cx.tick.ms;
            }
            3 => {
                self.interval = match self.interval {
                    3 => 5,
                    5 => 10,
                    _ => 3,
                }
            }
            _ => {}
        }
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
}
impl LogicalState for ViewerScreen {
    fn write(&self, c: &mut Canon) {
        c.str(&self.gallery)
            .u32(self.index as u32)
            .u32(self.generation)
            .bool(self.loading)
            .bool(self.slideshow)
            .u32(self.interval)
            .bool(self.covered)
            .str(&self.error);
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!(
            "image={}/{} slideshow={} interval={}",
            self.index,
            self.images.len(),
            self.slideshow,
            self.interval
        ));
    }
}
impl Focusable<StashHost> for ViewerScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        self.controls(|g| g.groups(cx, out))
    }
    fn group_of(&self, k: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        self.controls(|g| g.group_of(k, cx))
    }
    fn neighbour(&self, k: FocusKey<u32>, d: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        self.controls(|g| g.neighbour(k, d, cx))
    }
    fn place(&self, k: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        self.controls(|g| g.place(k, cx, at))
    }
    fn reconcile(&self, k: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        self.controls(|g| g.reconcile(k, cx))
    }
    fn seat(&self, g: GroupId, p: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        self.controls(|row| row.seat(g, p, cx))
    }
}
impl Machine<StashHost> for ViewerScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        ev: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        match ev {
            ScreenEvent::Mount => self.load(fx),
            ScreenEvent::Activate(k) => self.activate(*k, cx, fx),
            ScreenEvent::PressCommit(_) => {
                if let Some(k) = cx.focus.current {
                    self.activate(k.elem, cx, fx);
                }
            }
            ScreenEvent::Async(_, StashMsg::Loaded { generation, result })
                if *generation == self.generation =>
            {
                self.loading = false;
                match result {
                    Ok(data) => {
                        let prior = self.images.get(self.index).map(|i| i.id.clone());
                        self.images = data.images.clone();
                        self.index = prior
                            .and_then(|id| self.images.iter().position(|i| i.id == id))
                            .unwrap_or(self.index)
                            .min(self.images.len().saturating_sub(1));
                        self.last_slide = cx.tick.ms;
                        self.media(fx);
                    }
                    Err(e) => self.error = e.clone(),
                }
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Left | Key::Right,
                        edge: Edge::Down,
                        ..
                    },
                ..
            }) if cx.focus.current.is_none() => {
                self.advance(
                    if matches!(
                        ev,
                        ScreenEvent::Input(InputEvent {
                            kind: InputKind::Key { key: Key::Left, .. },
                            ..
                        })
                    ) {
                        -1
                    } else {
                        1
                    },
                    cx.tick.ms,
                    fx,
                );
            }
            ScreenEvent::Cover => self.covered = true,
            ScreenEvent::Uncover => {
                self.covered = false;
                self.last_slide = cx.tick.ms;
            }
            ScreenEvent::Tick(t)
                if self.slideshow
                    && !self.covered
                    && t.ms.wrapping_sub(self.last_slide) >= self.interval * 1000 =>
            {
                self.advance(1, t.ms, fx)
            }
            ScreenEvent::WillLeave(_) | ScreenEvent::Unmount => {
                self.slideshow = false;
                fx.push(Fx::App(StashFx::Media(Vec::new(), None)));
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
impl Screen<StashHost> for ViewerScreen {
    fn name(&self) -> &'static str {
        "StashViewer"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, _: &mut crate::ui::frame::Budget, _: &Cx<'_, StashHost>) {}
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let p = f.painter;
        if let Some(image) = self.images.get(self.index) {
            if let Some((tex, w, h)) = f.cx.views.textures.get(&format!("image:{}", image.id)) {
                let frame = Rect::new(0., 0., 1920., 916.);
                let scale = (frame.w / *w).min(frame.h / *h);
                p.tex(
                    *tex,
                    Rect::new(
                        frame.cx() - *w * scale / 2.,
                        frame.cy() - *h * scale / 2.,
                        *w * scale,
                        *h * scale,
                    ),
                    0.,
                    theme::TEXT_PRIMARY,
                );
            }
        }
        let labels = [
            if self.error.is_empty() {
                crate::i18n::msg::browse_gallery_previous().to_owned()
            } else {
                crate::i18n::msg::browse_action_retry().to_owned()
            },
            crate::i18n::msg::browse_gallery_next().into(),
            if self.slideshow {
                crate::i18n::msg::browse_gallery_slideshow_pause()
            } else {
                crate::i18n::msg::browse_gallery_slideshow_start()
            }
            .into(),
            crate::i18n::msg::browse_gallery_seconds(self.interval as i64),
        ];
        for (key, rect) in self.rects().into_iter().enumerate() {
            let focus = FocusKey {
                entry: self.entry,
                elem: key as u32,
            };
            f.stop(
                p,
                Stop {
                    key: focus,
                    rect,
                    rest_rect: rect,
                    clip: Rect::FULL,
                    hover: Hover::Focus,
                    activate: Activate::Press,
                },
            );
            let label = std::ffi::CString::new(labels[key].as_str()).unwrap();
            crate::ui::widgets::Button::new(label.as_ptr(), theme::size::BODY, rect)
                .focused(f.cx.focus.current == Some(focus))
                .draw(&Env::inert(), p);
        }
        let status = if self.loading {
            crate::i18n::msg::browse_library_loading().into()
        } else if !self.error.is_empty() {
            self.error.clone()
        } else if self.images.is_empty() {
            crate::i18n::msg::browse_gallery_empty().into()
        } else {
            format!("{} / {}", self.index + 1, self.images.len())
        };
        let status = std::ffi::CString::new(status.replace('\0', "")).unwrap();
        crate::ui::label::Label::new(status.as_ptr(), theme::size::BODY, theme::TEXT_SECONDARY)
            .draw(p, Rect::new(96., 54., 1728., 60.));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_images_cannot_replace_a_retried_gallery() {
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
        let mut screen = ViewerScreen::new(EntryId(1), "gallery".into(), 0);
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        screen.step(&ScreenEvent::Mount, &cx, &mut fx);
        screen.step(
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
        assert!(!screen.loading);
        screen.activate(0, &cx, &mut fx);
        assert!(screen.loading);
        assert_eq!(screen.generation, 2);
        let stale = crate::stores::stash::PageData {
            images: vec![Image {
                id: "stale".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(matches!(
            screen.step(
                &ScreenEvent::Async(
                    RequestId(1),
                    StashMsg::Loaded {
                        generation: 1,
                        result: Ok(stale)
                    }
                ),
                &cx,
                &mut fx
            ),
            Handled::No
        ));
        assert!(screen.images.is_empty());
    }
    #[test]
    fn slideshow_defaults_and_cover_pause_are_owned_by_the_viewer() {
        let _lock = crate::testlock::serial();
        let config = crate::stash::Config::default();
        let textures = std::collections::HashMap::new();
        let playback = PlaybackView::default();
        let mut cx = Cx {
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
        let mut screen = ViewerScreen::new(EntryId(1), "gallery".into(), 0);
        screen.images = vec![
            Image {
                id: "1".into(),
                ..Default::default()
            },
            Image {
                id: "2".into(),
                ..Default::default()
            },
        ];
        assert_eq!(screen.interval, 5);
        assert!(!screen.slideshow);
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        screen.activate(2, &cx, &mut fx);
        cx.tick.ms = 4999;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 0);
        cx.tick.ms = 5000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 1);
        screen.step(&ScreenEvent::Cover, &cx, &mut fx);
        cx.tick.ms = 10000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 1);
        screen.step(&ScreenEvent::Uncover, &cx, &mut fx);
        cx.tick.ms = 14999;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 1);
        cx.tick.ms = 15000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 0);
        screen.activate(2, &cx, &mut fx);
        cx.tick.ms = 20000;
        screen.step(&ScreenEvent::Tick(cx.tick), &cx, &mut fx);
        assert_eq!(screen.index, 0);
        for expected in [10, 3, 5] {
            screen.activate(3, &cx, &mut fx);
            assert_eq!(screen.interval, expected);
        }
        screen.advance(-1, cx.tick.ms, &mut fx);
        assert_eq!(screen.index, 1);
        screen.advance(1, cx.tick.ms, &mut fx);
        assert_eq!(screen.index, 0);
    }
}
