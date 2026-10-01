//! Performer biography over the shared collection layout; tag aggregation is paged off-frame.
use super::catalog::StashScreen;
use crate::screens::stash_registry::*;
use crate::stash::{SceneTagSummary, Tag};
use crate::stores::stash::Work;
use crate::ui::consts::{MARGIN_X, SCR_W};
use crate::ui::hero_content;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::text_view::TextView;
use crate::ui::widgets::Button;
use crate::ui::{theme, Env, Rect, View};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

const HERO: GroupId = GroupId(0);
const SCENES: u32 = 0x1000_0010;
const RETRY_TAGS: u32 = SCENES + 1;
const PORTRAIT_W: f32 = crate::ui::card_row::RowStyle::HOME.w;
const PORTRAIT_H: f32 = crate::ui::card_row::RowStyle::HOME.h;
const TEXT_X: f32 = MARGIN_X + PORTRAIT_W + theme::space::XL;
const TEXT_W: f32 = SCR_W - MARGIN_X - TEXT_X;

#[derive(Default)]
struct PopularTags {
    scenes: HashSet<String>,
    counts: HashMap<String, (Tag, usize)>,
}
impl PopularTags {
    fn add(&mut self, scenes: &[SceneTagSummary]) {
        for scene in scenes {
            if !self.scenes.insert(scene.id.clone()) {
                continue;
            }
            let mut seen = HashSet::new();
            for tag in &scene.tags {
                if !seen.insert(&tag.id) {
                    continue;
                }
                let count = self
                    .counts
                    .entry(tag.id.clone())
                    .or_insert((tag.clone(), 0));
                count.1 += 1;
            }
        }
    }
    fn top(&self) -> Vec<Tag> {
        let mut tags: Vec<_> = self.counts.values().collect();
        tags.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        tags.into_iter()
            .take(5)
            .map(|(tag, _)| tag.clone())
            .collect()
    }
}

pub struct PerformerScreen {
    entry: EntryId,
    id: String,
    content: StashScreen,
    popular: PopularTags,
    top: Vec<Tag>,
    generation: u32,
    page: u32,
    loading_tags: bool,
    tags_started: bool,
    tags_error: bool,
    hero_seated: bool,
}
impl PerformerScreen {
    pub fn new(id: String, entry: EntryId) -> Self {
        Self {
            entry,
            content: StashScreen::new(StashArg::Performer(id.clone()), entry),
            id,
            popular: PopularTags::default(),
            top: Vec::new(),
            generation: 1,
            page: 1,
            loading_tags: false,
            tags_started: false,
            tags_error: false,
            hero_seated: false,
        }
    }
    fn request_tags(&mut self, fx: &mut Effects<'_, StashHost>) {
        self.loading_tags = true;
        self.tags_error = false;
        fx.push(Fx::App(StashFx::Work(
            Addr {
                to: fx.from(),
                req: RequestId(self.generation),
            },
            Work::PerformerTags {
                performer_id: self.id.clone(),
                page: self.page,
                generation: self.generation,
            },
        )));
    }
    fn title<'a>(&'a self, cx: &'a Cx<'_, StashHost>) -> TextView<'a> {
        TextView::new(
            self.content
                .performer()
                .map(|p| p.name.as_str())
                .unwrap_or(""),
            theme::size::HERO,
            theme::TEXT_PRIMARY,
        )
        .with_measure(cx.measure)
        .max_lines(2)
    }
    fn description<'a>(&'a self, cx: &'a Cx<'_, StashHost>) -> TextView<'a> {
        TextView::new(
            self.content
                .performer()
                .and_then(|p| p.details.as_deref())
                .unwrap_or(""),
            theme::size::LABEL,
            theme::TEXT_SECONDARY,
        )
        .with_measure(cx.measure)
        .max_lines(3)
    }
    fn facts(&self) -> String {
        let Some(p) = self.content.performer() else {
            return String::new();
        };
        let end = p
            .death_date
            .as_deref()
            .and_then(parse_date)
            .or(self.content.reference_date());
        let mut facts = Vec::new();
        if let Some(age) = p.birthdate.as_deref().and_then(|birth| age_at(birth, end?)) {
            facts.push(crate::i18n::msg::stash_performer_age(age as i64));
        }
        if let Some(hair) = p.hair_color.as_deref().filter(|s| !s.is_empty()) {
            facts.push(crate::i18n::msg::stash_performer_hair(hair));
        }
        facts.join(" · ")
    }
    fn badges(&self, cx: &Cx<'_, StashHost>, tags: &[Tag]) -> hero_content::PassiveBadges {
        let labels = tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>();
        hero_content::PassiveBadges::new(&labels, TEXT_W, cx.measure)
    }
    fn layout(&self, cx: &Cx<'_, StashHost>) -> (f32, f32, f32, f32, f32) {
        let top = crate::ui::widgets::TOP_BAR_BOTTOM + theme::space::XL;
        let description = top + self.title(cx).measure_h(TEXT_W) + theme::space::MD;
        let facts = description + self.description(cx).measure_h(TEXT_W) + theme::space::MD;
        let models = facts + cx.measure.cap_h(theme::size::CAPTION) + theme::space::MD;
        let model_end = self
            .content
            .performer()
            .map(|p| models + self.badges(cx, &p.tags).height())
            .unwrap_or(models);
        let popular = model_end + theme::space::MD;
        // Reserve the final two tag rows before aggregation lands, keeping the focused grid still.
        let last = popular
            + cx.measure.cap_h(theme::size::CAPTION)
            + theme::space::SM
            + crate::ui::widgets::BADGE_H * 2.
            + theme::space::SM;
        let actions = last.max(top + PORTRAIT_H) + theme::space::LG;
        (description, facts, models, popular, actions)
    }
    fn rect(&self, cx: &Cx<'_, StashHost>) -> Rect {
        let label = crate::i18n::msg::browse_stash_scenes_c();
        Rect::new(
            TEXT_X,
            self.layout(cx).4 - self.content.scroll(),
            Button::pill_w(label.as_ptr(), theme::size::BODY, false),
            crate::ui::widgets::StatusOverlay::CTRL_H,
        )
    }
    fn activate(&self, key: u32, cx: &Cx<'_, StashHost>, fx: &mut Effects<'_, StashHost>) -> bool {
        if key != SCENES {
            return false;
        }
        if let Some(group) = self.content.first_group() {
            let seat = self.content.seat(
                group,
                Placed {
                    rect: self.rect(cx),
                    rest_rect: self.rect(cx),
                    clip: Rect::FULL,
                    index: None,
                },
                cx,
            );
            fx.push(Fx::Deliver(
                fx.from(),
                Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                    focus: FocusTarget::Elem(seat),
                })),
            ));
        }
        true
    }
}
impl LogicalState for PerformerScreen {
    fn write(&self, c: &mut Canon) {
        self.content.state().write(c);
        c.str(&self.id)
            .u32(self.generation)
            .u32(self.page)
            .bool(self.loading_tags)
            .bool(self.tags_error);
        for tag in &self.top {
            c.str(&tag.id);
        }
    }
    fn probe(&self, s: &mut String) {
        self.content.state().probe(s);
        s.push_str(&format!(
            " tags-page={} tags-loading={}",
            self.page, self.loading_tags
        ));
    }
}
impl Focusable<StashHost> for PerformerScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        if self.content.performer().is_some() {
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
                extent: self.rect(cx),
                len: if self.tags_error { 2 } else { 1 },
                elem: ElemKind::Control,
            });
        }
        self.content.groups(cx, out);
    }
    fn group_of(&self, key: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        if (*key == SCENES || (*key == RETRY_TAGS && self.tags_error))
            && self.content.performer().is_some()
        {
            Some(HERO)
        } else {
            self.content.group_of(key, cx)
        }
    }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        if key.elem == SCENES && self.tags_error && dir == Dir::Right {
            Step::Move(FocusKey {
                entry: self.entry,
                elem: RETRY_TAGS,
            })
        } else if key.elem == RETRY_TAGS && dir == Dir::Left {
            Step::Move(FocusKey {
                entry: self.entry,
                elem: SCENES,
            })
        } else if key.elem == SCENES || key.elem == RETRY_TAGS {
            Step::Edge
        } else {
            self.content.neighbour(key, dir, cx)
        }
    }
    fn place(&self, key: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        if self.group_of(key, cx) == Some(HERO) {
            let rect = if *key == RETRY_TAGS {
                let scenes = self.rect(cx);
                Rect::new(
                    scenes.x + scenes.w + theme::space::MD,
                    scenes.y,
                    Button::pill_w(
                        crate::i18n::msg::browse_action_retry_c().as_ptr(),
                        theme::size::BODY,
                        false,
                    ),
                    scenes.h,
                )
            } else {
                self.rect(cx)
            };
            Some(Placed {
                rect,
                rest_rect: rect,
                clip: Rect::FULL,
                index: Some(key - SCENES),
            })
        } else {
            self.content.place(key, cx, at)
        }
    }
    fn reconcile(&self, key: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if self.group_of(&key.elem, cx) == Some(HERO) {
            key
        } else if key.elem == RETRY_TAGS {
            FocusKey {
                entry: self.entry,
                elem: SCENES,
            }
        } else {
            self.content.reconcile(key, cx)
        }
    }
    fn seat(&self, group: GroupId, from: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        if group == HERO {
            FocusKey {
                entry: self.entry,
                elem: SCENES,
            }
        } else {
            self.content.seat(group, from, cx)
        }
    }
}
impl Machine<StashHost> for PerformerScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        event: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        self.content.detail_inset(
            self.layout(cx).4 + crate::ui::widgets::StatusOverlay::CTRL_H + theme::space::XL,
        );
        let collapsed =
            hero_content::collapse_fraction(self.content.scroll(), self.layout(cx).4) > 0.;
        self.content.hit_clearance(if collapsed {
            hero_content::pinned_name_bottom(cx.measure)
        } else {
            0.
        });
        let images = self
            .content
            .performer()
            .filter(|_| self.content.scroll() < self.layout(cx).4)
            .and_then(|p| {
                p.image_path.as_ref().map(|url| {
                    vec![
                        (format!("still:performer:{}", p.id), url.clone()),
                        (format!("blur:performer:{}", p.id), url.clone()),
                    ]
                })
            })
            .unwrap_or_default();
        self.content.media_override(images, None);
        match event {
            ScreenEvent::Activate(key) if *key == RETRY_TAGS && self.tags_error => {
                self.request_tags(fx);
                fx.invalidate(crate::ui::present::Provenance::Input);
                return Handled::Yes;
            }
            ScreenEvent::PressCommit(_)
                if cx.focus.current.is_some_and(|k| k.elem == RETRY_TAGS) && self.tags_error =>
            {
                self.request_tags(fx);
                fx.invalidate(crate::ui::present::Provenance::Input);
                return Handled::Yes;
            }
            ScreenEvent::Async(
                _,
                StashMsg::PerformerTags {
                    generation,
                    page,
                    result,
                },
            ) if *generation == self.generation && *page == self.page && self.loading_tags => {
                self.loading_tags = false;
                match result {
                    Ok(page) => {
                        self.popular.add(&page.items);
                        if !page.items.is_empty() && (self.page as usize) * 50 < page.count {
                            self.page += 1;
                            self.request_tags(fx);
                        } else {
                            self.top = self.popular.top();
                        }
                    }
                    Err(_) => self.tags_error = true,
                }
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                return Handled::Yes;
            }
            ScreenEvent::Uncover if self.tags_error => self.request_tags(fx),
            ScreenEvent::Activate(key) if self.activate(*key, cx, fx) => return Handled::Yes,
            ScreenEvent::PressCommit(_)
                if cx
                    .focus
                    .current
                    .is_some_and(|k| self.activate(k.elem, cx, fx)) =>
            {
                return Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } if to.elem == SCENES || to.elem == RETRY_TAGS => {
                self.content.shelf_scroll(0.);
                fx.invalidate(crate::ui::present::Provenance::Input);
                return Handled::Yes;
            }
            _ => {}
        }
        let result = self.content.step(event, cx, fx);
        if !self.hero_seated && self.content.performer().is_some() {
            self.hero_seated = true;
            self.content.shelf_scroll(0.);
            fx.push(Fx::Deliver(
                fx.from(),
                Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                    focus: FocusTarget::Elem(FocusKey {
                        entry: self.entry,
                        elem: SCENES,
                    }),
                })),
            ));
        }
        if !self.tags_started && self.content.performer().is_some() {
            self.tags_started = true;
            self.request_tags(fx);
        }
        result
    }
}
impl Screen<StashHost> for PerformerScreen {
    fn name(&self) -> &'static str {
        "performer"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, cx: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        self.content.crumb(cx)
    }
    fn prepare(&mut self, b: &mut crate::ui::frame::Budget, cx: &Cx<'_, StashHost>) {
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
        let collapse = hero_content::collapse_fraction(self.content.scroll(), self.layout(f.cx).4);
        if let Some(performer) = self.content.performer() {
            let scroll = self.content.scroll();
            let a = (1. - scroll / self.layout(f.cx).4).clamp(0., 1.);
            let p = f.painter.alpha(a);
            if let Some(&(texture, w, h)) =
                f.cx.views
                    .textures
                    .get(&format!("blur:performer:{}", performer.id))
            {
                p.tex_uv(
                    texture,
                    Rect::FULL.cover_uv(w, h, crate::ui::Crop::Centre),
                    Rect::FULL,
                    0.,
                    theme::TEXT_PRIMARY,
                );
            }
            p.rect(
                Rect::FULL,
                0.,
                theme::scrim_black(0.6),
                theme::scrim_black(0.6),
                0.,
            );
            crate::ui::widgets::hero_scrim(p, a, false);
            if let Some(&(texture, w, h)) =
                f.cx.views
                    .textures
                    .get(&format!("still:performer:{}", performer.id))
            {
                let box_rect = Rect::new(
                    MARGIN_X,
                    crate::ui::widgets::TOP_BAR_BOTTOM + theme::space::XL - scroll,
                    PORTRAIT_W,
                    PORTRAIT_H,
                );
                let scale = (box_rect.w / w).min(box_rect.h / h);
                let rect = Rect::new(
                    box_rect.x + (box_rect.w - w * scale) * 0.5,
                    box_rect.y + (box_rect.h - h * scale) * 0.5,
                    w * scale,
                    h * scale,
                );
                p.tex(texture, rect, theme::CARD_RING_RAD, theme::TEXT_PRIMARY);
            }
            self.title(f.cx).draw(
                p,
                Rect::new(
                    TEXT_X + (MARGIN_X - TEXT_X) * collapse,
                    hero_content::pinned_title_y(
                        crate::ui::widgets::TOP_BAR_BOTTOM + theme::space::XL,
                        scroll,
                        crate::ui::widgets::TOP_BAR_BOTTOM + theme::space::MD,
                    ),
                    TEXT_W,
                    0.,
                ),
            );
            let (description, facts, models, popular, _) = self.layout(f.cx);
            self.description(f.cx)
                .draw(p, Rect::new(TEXT_X, description - scroll, TEXT_W, 0.));
            let fact_text = self.facts();
            TextView::new(&fact_text, theme::size::CAPTION, theme::TEXT_SECONDARY)
                .with_measure(f.cx.measure)
                .max_lines(1)
                .draw(p, Rect::new(TEXT_X, facts - scroll, TEXT_W, 0.));
            self.badges(f.cx, &performer.tags)
                .draw(p, TEXT_X, models - scroll, f.cx.measure);
            if !self.top.is_empty() {
                TextView::new(
                    crate::i18n::msg::stash_performer_popular_tags(),
                    theme::size::CAPTION,
                    theme::TEXT_SECONDARY,
                )
                .with_measure(f.cx.measure)
                .max_lines(1)
                .draw(p, Rect::new(TEXT_X, popular - scroll, TEXT_W, 0.));
                self.badges(f.cx, &self.top).draw(
                    p,
                    TEXT_X,
                    popular + f.cx.measure.cap_h(theme::size::CAPTION) + theme::space::SM - scroll,
                    f.cx.measure,
                );
            }
            if self.tags_error {
                TextView::new(
                    crate::i18n::msg::stash_performer_tags_error(),
                    theme::size::CAPTION,
                    theme::TEXT_SECONDARY,
                )
                .with_measure(f.cx.measure)
                .max_lines(1)
                .draw(p, Rect::new(TEXT_X, popular - scroll, TEXT_W, 0.));
                let key = FocusKey {
                    entry: self.entry,
                    elem: RETRY_TAGS,
                };
                let rect = self.place(&RETRY_TAGS, f.cx, At::Drawn).unwrap().rect;
                f.stop(
                    p,
                    Stop {
                        key,
                        rect,
                        rest_rect: rect,
                        clip: if collapse > 0. {
                            Rect::new(
                                0.,
                                hero_content::pinned_name_bottom(f.cx.measure),
                                SCR_W,
                                crate::ui::consts::SCR_H
                                    - hero_content::pinned_name_bottom(f.cx.measure),
                            )
                        } else {
                            Rect::FULL
                        },
                        hover: Hover::Focus,
                        activate: Activate::Press,
                    },
                );
                Button::new(
                    crate::i18n::msg::browse_action_retry_c().as_ptr(),
                    theme::size::BODY,
                    rect,
                )
                .focused(f.cx.focus.current == Some(key))
                .draw(&Env::inert(), p);
            }
            let rect = self.rect(f.cx);
            let key = FocusKey {
                entry: self.entry,
                elem: SCENES,
            };
            if rect.y + rect.h > crate::ui::detail_layout::TOP_MARGIN {
                f.stop(
                    p,
                    Stop {
                        key,
                        rect,
                        rest_rect: rect,
                        clip: if collapse > 0. {
                            Rect::new(
                                0.,
                                hero_content::pinned_name_bottom(f.cx.measure),
                                SCR_W,
                                crate::ui::consts::SCR_H
                                    - hero_content::pinned_name_bottom(f.cx.measure),
                            )
                        } else {
                            Rect::FULL
                        },
                        hover: Hover::Focus,
                        activate: Activate::Press,
                    },
                );
                Button::new(
                    crate::i18n::msg::browse_stash_scenes_c().as_ptr(),
                    theme::size::BODY,
                    rect,
                )
                .focused(f.cx.focus.current == Some(key))
                .draw(&Env::inert(), p);
            }
        }
        self.content.draw(f);
        if let Some(performer) = self.content.performer() {
            hero_content::draw_pinned_name(f.painter, &performer.name, collapse, f.cx.measure);
        }
    }
}

fn parse_date(date: &str) -> Option<(i32, u32, u32)> {
    let mut parts = date.split('-');
    let (year, month, day): (i32, u32, u32) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() || !(1..=9999).contains(&year) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    (1..=last).contains(&day).then_some((year, month, day))
}
fn age_at(birth: &str, today: (i32, u32, u32)) -> Option<i32> {
    let birth = parse_date(birth)?;
    let age = today.0 - birth.0 - i32::from((today.1, today.2) < (birth.1, birth.2));
    (0..=130).contains(&age).then_some(age)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn popular_tags_count_distinct_scenes_and_tags_across_pages() {
        let tag = |id: &str, name: &str| Tag {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        };
        let a = tag("a", "Alpha");
        let b = tag("b", "Beta");
        let mut tags = PopularTags::default();
        tags.add(&[SceneTagSummary {
            id: "1".into(),
            tags: vec![b.clone(), b.clone(), a.clone()],
        }]);
        tags.add(&[
            SceneTagSummary {
                id: "1".into(),
                tags: vec![a.clone()],
            },
            SceneTagSummary {
                id: "51".into(),
                tags: vec![b],
            },
        ]);
        assert_eq!(
            tags.top().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(tags.counts["b"].1, 2);
    }
    #[test]
    fn age_respects_birthday_and_missing_dates() {
        assert_eq!(age_at("1994-08-20", (2026, 8, 19)), Some(31));
        assert_eq!(age_at("1994-08-20", (2026, 8, 20)), Some(32));
        assert_eq!(age_at("", (2026, 8, 20)), None);
        assert_eq!(age_at("2027-01-01", (2026, 8, 20)), None);
    }
    #[test]
    fn dates_reject_impossible_days_and_respect_gregorian_leap_years() {
        assert_eq!(parse_date("2024-02-29"), Some((2024, 2, 29)));
        for date in [
            "2023-02-29",
            "2024-02-31",
            "1900-02-29",
            "2024-04-31",
            "0000-01-01",
            "2024-13-01",
            "2024-01-00",
        ] {
            assert!(parse_date(date).is_none(), "{date}");
        }
        assert_eq!(parse_date("2000-02-29"), Some((2000, 2, 29)));
    }
    #[test]
    fn asynchronous_popular_tags_preserve_hero_height_and_reject_stale_generations() {
        let _guard = crate::testlock::serial();
        let textures = HashMap::new();
        let config = crate::stash::Config::default();
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
        let mut screen = PerformerScreen::new("1".into(), EntryId(1));
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        screen.step(&ScreenEvent::Mount, &cx, &mut fx);
        screen.step(
            &ScreenEvent::Async(
                RequestId(1),
                StashMsg::Loaded {
                    generation: 1,
                    result: Ok(crate::stores::stash::PageData {
                        performer: Some(crate::stash::Performer {
                            id: "1".into(),
                            name: "Fixture".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ),
            &cx,
            &mut fx,
        );
        let height = screen.layout(&cx).4;
        assert!(screen.hero_seated);
        assert!(out.iter().any(|event| matches!(&event.fx,
            Fx::Deliver(_, Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                focus: FocusTarget::Elem(key),
            }))) if key.elem == SCENES)));
        let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
        let page = || crate::stash::Page {
            count: 1,
            items: vec![SceneTagSummary {
                id: "1".into(),
                tags: vec![Tag {
                    id: "t".into(),
                    name: "Tag".into(),
                    ..Default::default()
                }],
            }],
        };
        screen.step(
            &ScreenEvent::Async(
                RequestId(99),
                StashMsg::PerformerTags {
                    generation: 99,
                    page: 1,
                    result: Ok(page()),
                },
            ),
            &cx,
            &mut fx,
        );
        assert!(screen.top.is_empty());
        assert!(screen.loading_tags);
        screen.step(
            &ScreenEvent::Async(
                RequestId(1),
                StashMsg::PerformerTags {
                    generation: 1,
                    page: 1,
                    result: Ok(page()),
                },
            ),
            &cx,
            &mut fx,
        );
        assert_eq!(screen.top.len(), 1);
        assert!(!screen.loading_tags);
        assert_eq!(screen.layout(&cx).4, height);
    }
}
