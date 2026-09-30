//! Connection settings own their editing and asynchronous save lifecycle.
use crate::screens::stash_registry::*;
use crate::stash::Config;
use crate::stores::stash::Work;
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::table::{Row, Section, TableView};
use crate::ui::{theme, Rect};
use std::borrow::Cow;
const FRAME: Rect = Rect::new(96., 200., 1728., 720.);
pub struct SettingsScreen {
    entry: EntryId,
    url: String,
    key: String,
    editing: Option<u32>,
    caret: usize,
    generation: u32,
    loading: bool,
    status: String,
    table: TableView,
}
impl SettingsScreen {
    pub fn new(entry: EntryId) -> Self {
        Self {
            entry,
            url: String::new(),
            key: String::new(),
            editing: None,
            caret: 0,
            generation: 0,
            loading: false,
            status: String::new(),
            table: TableView::new(),
        }
    }
    fn rebuild(&mut self) {
        let selected = self.table.sel;
        self.table.set_sections(
            vec![Section::new("")
                .row(
                    Row::new(crate::i18n::msg::settings_stash_server_url()).value(self.url.clone()),
                )
                .row(Row::new(crate::i18n::msg::settings_stash_api_key()).value(
                    if self.key.is_empty() {
                        crate::i18n::msg::settings_stash_optional()
                    } else {
                        "••••••••"
                    },
                ))
                .row(Row::new(if self.loading {
                    crate::i18n::msg::settings_stash_testing()
                } else if self.status.is_empty() {
                    crate::i18n::msg::settings_stash_test_save()
                } else {
                    crate::i18n::msg::settings_stash_test_again()
                }))],
            selected,
            true,
        );
    }
    fn table(&self) -> crate::ui::geom::Table<'_> {
        crate::ui::geom::Table {
            table: &self.table,
            frame: FRAME,
            group: GroupId(0),
            entry: self.entry,
        }
    }
    fn edit(&mut self, edit: &TextEdit) {
        if let Some(field) = self.editing {
            let value = if field == 0 {
                &mut self.url
            } else {
                &mut self.key
            };
            let mut buffer = crate::ui::text_buffer::TextBuffer::new(value.clone(), self.caret);
            buffer.edit(edit);
            self.caret = buffer.caret();
            *value = buffer.into_text();
            self.rebuild();
        }
    }
    fn activate(&mut self, key: u32, fx: &mut Effects<'_, StashHost>) {
        if let Some(arg) = strip_destination(key) {
            fx.push(Fx::Nav(NavOp::Root(arg)));
            return;
        }
        if key < 2 {
            self.editing = Some(key);
            self.caret = if key == 0 {
                self.url.len()
            } else {
                self.key.len()
            };
            fx.push(Fx::App(StashFx::Keyboard(true)));
        } else if key == 2 && !self.loading {
            self.generation += 1;
            self.loading = true;
            self.status.clear();
            fx.push(Fx::App(StashFx::Work(
                Addr {
                    to: fx.from(),
                    req: RequestId(self.generation),
                },
                Work::Connect(Config {
                    server_url: self.url.clone(),
                    api_key: self.key.clone(),
                }),
            )));
            self.rebuild();
        }
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
}
impl LogicalState for SettingsScreen {
    fn write(&self, c: &mut Canon) {
        c.str(&self.url)
            .u32(self.generation)
            .bool(self.loading)
            .u32(self.editing.unwrap_or(3))
            .u32(self.caret as u32)
            .str(&self.status);
    }
    fn probe(&self, s: &mut String) {
        s.push_str(&format!(
            "connection loading={} editing={:?}",
            self.loading, self.editing
        ));
    }
}
impl Focusable<StashHost> for SettingsScreen {
    fn groups(&self, cx: &Cx<'_, StashHost>, out: &mut Vec<GroupSpec>) {
        self.table().groups(cx, out)
    }
    fn group_of(&self, k: &u32, cx: &Cx<'_, StashHost>) -> Option<GroupId> {
        self.table().group_of(k, cx)
    }
    fn neighbour(&self, k: FocusKey<u32>, d: Dir, cx: &Cx<'_, StashHost>) -> Step<u32> {
        self.table().neighbour(k, d, cx)
    }
    fn place(&self, k: &u32, cx: &Cx<'_, StashHost>, at: At) -> Option<Placed> {
        self.table().place(k, cx, at)
    }
    fn reconcile(&self, k: FocusKey<u32>, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        self.table().reconcile(k, cx)
    }
    fn seat(&self, g: GroupId, p: Placed, cx: &Cx<'_, StashHost>) -> FocusKey<u32> {
        self.table().seat(g, p, cx)
    }
}
impl Machine<StashHost> for SettingsScreen {
    type Ev = ScreenEvent<StashHost>;
    fn step(
        &mut self,
        ev: &Self::Ev,
        cx: &Cx<'_, StashHost>,
        fx: &mut Effects<'_, StashHost>,
    ) -> Handled {
        match ev {
            ScreenEvent::Mount => {
                self.url = cx.views.config.server_url.clone();
                self.key = cx.views.config.api_key.clone();
                self.rebuild();
            }
            ScreenEvent::Activate(k) => self.activate(*k, fx),
            ScreenEvent::PressCommit(_) => {
                if let Some(k) = cx.focus.current {
                    self.activate(k.elem, fx);
                }
            }
            ScreenEvent::FocusMoved { to, .. } => {
                self.table.sel = to.elem as i32;
            }
            ScreenEvent::Input(InputEvent {
                kind: InputKind::Text(edit),
                ..
            }) if self.editing.is_some() => {
                self.edit(edit);
                fx.invalidate(crate::ui::present::Provenance::Input);
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Left | Key::Right,
                        edge: Edge::Down | Edge::Repeat,
                        ..
                    },
                ..
            }) if self.editing.is_some() => {
                self.edit(
                    if matches!(
                        ev,
                        ScreenEvent::Input(InputEvent {
                            kind: InputKind::Key { key: Key::Left, .. },
                            ..
                        })
                    ) {
                        &TextEdit::Left
                    } else {
                        &TextEdit::Right
                    },
                );
            }
            ScreenEvent::Input(InputEvent {
                kind:
                    InputKind::Key {
                        key: Key::Ok | Key::Back,
                        edge: Edge::Down,
                        ..
                    },
                ..
            }) if self.editing.is_some() => {
                self.editing = None;
                fx.push(Fx::App(StashFx::Keyboard(false)));
            }
            ScreenEvent::Async(_, StashMsg::Connected(result)) => {
                self.loading = false;
                self.status = match result {
                    Ok(_) => crate::i18n::msg::settings_stash_connection_saved().into(),
                    Err(e) => e.clone(),
                };
                self.rebuild();
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
            }
            ScreenEvent::Tick(t) => {
                self.table.list_focused = cx.focus.current.is_some_and(|k| k.elem < 3);
                self.table.sel = cx
                    .focus
                    .current
                    .filter(|k| k.elem < 3)
                    .map_or(self.table.sel, |k| k.elem as i32);
                self.table.update(t.dt(), FRAME.h);
            }
            ScreenEvent::WillLeave(_) | ScreenEvent::Unmount => {
                self.editing = None;
                fx.push(Fx::App(StashFx::Keyboard(false)));
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
impl Screen<StashHost> for SettingsScreen {
    fn name(&self) -> &'static str {
        "StashSettings"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _: &Cx<'_, StashHost>) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed("Settings"))
    }
    fn prepare(&mut self, _: &mut crate::ui::frame::Budget, _: &Cx<'_, StashHost>) {}
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, StashHost>) {
        let p = f.painter;
        let title = crate::i18n::msg::settings_title_c();
        crate::ui::label::Label::new(title.as_ptr(), theme::size::TITLE, theme::TEXT_PRIMARY)
            .draw(p, Rect::new(96., 100., 1728., 65.));
        self.table.draw(p, FRAME, f.cx.measure);
        for key in 0..3 {
            if let Some(place) = self.place(&key, f.cx, At::Drawn) {
                f.stop(
                    p,
                    Stop {
                        key: FocusKey {
                            entry: self.entry,
                            elem: key,
                        },
                        rect: place.rect,
                        rest_rect: place.rest_rect,
                        clip: place.clip,
                        hover: Hover::Focus,
                        activate: Activate::Press,
                    },
                );
            }
        }
        let status = std::ffi::CString::new(self.status.replace('\0', "")).unwrap();
        crate::ui::label::Label::new(status.as_ptr(), theme::size::BODY, theme::TEXT_SECONDARY)
            .draw(p, Rect::new(96., 940., 1728., 70.));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_preserves_utf8_and_masks_key_while_connect_requests_do_not_overlap() {
        let _lock = crate::testlock::serial();
        let config = Config {
            server_url: "http://fixture.invalid".into(),
            api_key: "secret😀".into(),
        };
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
        let mut screen = SettingsScreen::new(EntryId(1));
        let mut out = Vec::new();
        let mut present = crate::ui::present::Present::new();
        {
            let mut fx = Effects::new(&mut out, MachineId::Instance(InstanceId(1)), &mut present);
            screen.step(&ScreenEvent::Mount, &cx, &mut fx);
            screen.activate(1, &mut fx);
            screen.edit(&TextEdit::Backspace);
            assert_eq!(screen.key, "secret");
            assert!(!screen.table.sections[0].rows[1]
                .value
                .as_deref()
                .unwrap_or("")
                .contains("secret"));
            screen.editing = None;
            screen.activate(2, &mut fx);
            screen.activate(2, &mut fx);
        }
        assert_eq!(
            out.iter()
                .filter(|effect| matches!(effect.fx, Fx::App(StashFx::Work(_, Work::Connect(_)))))
                .count(),
            1
        );
        assert!(screen.loading);
    }
}
