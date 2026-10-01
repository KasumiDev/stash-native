//! Dedicated Stash vocabulary: no Plex identifiers cross this boundary.
use crate::stash::{Config, Scene};
use crate::stores::stash::Work;
pub use crate::stores::stash::{Action, StashArg, StashMsg};
use crate::ui::machine::*;
use crate::ui::screen::{Mounter, ReturnState, Screen, ScreenArg};
impl ScreenArg for StashArg {
    fn chrome(&self) -> Chrome {
        match self {
            Self::Home
            | Self::Performers
            | Self::Scenes
            | Self::Galleries
            | Self::Tags
            | Self::Search => Chrome::TabBar,
            _ => Chrome::None,
        }
    }
    fn id(&self) -> ScreenId {
        ScreenId(match self {
            Self::Home => 1,
            Self::Performers => 2,
            Self::Scenes => 3,
            Self::Galleries => 4,
            Self::Tags => 5,
            Self::Search => 6,
            Self::Settings => 7,
            Self::Scene(_) => 8,
            Self::Performer(_) => 9,
            Self::Tag(_) => 10,
            Self::Gallery(_) => 11,
            Self::Viewer { .. } => 12,
            Self::Player(_) => 13,
        })
    }
    fn title(&self) -> Option<&str> {
        Some(self.label())
    }
    fn same_instance(&self, other: &Self) -> bool {
        self == other
    }
}
impl LogicalState for StashArg {
    fn write(&self, c: &mut Canon) {
        c.str(&format!("{self:?}"));
    }
    fn probe(&self, s: &mut String) {
        s.push_str(self.label());
    }
}
pub struct StashHost;
pub(crate) const STASH_ACTIVITY: StoreOrd = StoreOrd(0x5354_4153);
/// Shared strip identities are navigation actions, never provider content identifiers.
pub(crate) fn strip_destination(key: u32) -> Option<StashArg> {
    let index = key.checked_sub(crate::ui::dispatch::STRIP_BASE)?;
    match index {
        0 => Some(StashArg::Home),
        1 => Some(StashArg::Performers),
        2 => Some(StashArg::Scenes),
        3 => Some(StashArg::Galleries),
        4 => Some(StashArg::Tags),
        5 => Some(StashArg::Search),
        6 => Some(StashArg::Settings),
        _ => None,
    }
}
/// Read-only native playback projection; the transport remains application-owned.
#[derive(Clone, Debug, Default)]
pub struct PlaybackView {
    pub scene: Option<Scene>,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub loading: bool,
    pub completed: bool,
    pub o_count: i64,
    pub o_pending: bool,
    pub error: String,
}
#[derive(Default)]
pub struct Init;
impl LogicalState for Init {
    fn write(&self, _: &mut Canon) {}
    fn probe(&self, _: &mut String) {}
}

pub enum StashFx {
    Work(Addr, Work),
    Media(Vec<(String, String)>, Option<(String, String)>),
    Play(Scene, bool),
    Player(Action),
    Keyboard(bool),
}

#[derive(Clone, Copy)]
pub struct Views<'a> {
    pub textures: &'a std::collections::HashMap<String, (u32, f32, f32)>,
    pub config: &'a Config,
    pub playback: &'a PlaybackView,
}
impl Host for StashHost {
    type Arg = StashArg;
    type Fx = StashFx;
    type Msg = StashMsg;
    type Elem = u32;
    type Views<'a> = Views<'a>;
    type Init = Init;
    type Memory = ();
}
pub struct Mount;
impl Mounter<StashHost> for Mount {
    fn mount(
        &mut self,
        _: InstanceId,
        arg: &StashArg,
        _: &ReturnState<u32>,
        cx: &Cx<'_, StashHost>,
        _: &mut Effects<'_, StashHost>,
    ) -> Box<dyn Screen<StashHost>> {
        let entry = match cx.owner {
            InputOwner::Entry(e) => e,
            _ => EntryId(0),
        };
        match arg {
            StashArg::Home => Box::new(crate::screens::stash::home::HomeScreen::new(entry)),
            StashArg::Scene(id) => Box::new(crate::screens::stash::SceneDetailScreen::new(
                id.clone(),
                entry,
            )),
            StashArg::Performer(id) => Box::new(crate::screens::stash::PerformerScreen::new(id.clone(), entry)),
            StashArg::Player(id) => Box::new(crate::screens::stash::player::PlayerScreen::new(
                id.clone(),
                entry,
            )),
            StashArg::Settings => Box::new(crate::screens::stash::SettingsScreen::new(entry)),
            StashArg::Viewer { gallery, index } => Box::new(
                crate::screens::stash::ViewerScreen::new(entry, gallery.clone(), *index),
            ),
            _ => Box::new(crate::screens::stash::StashScreen::new(arg.clone(), entry)),
        }
    }
}
