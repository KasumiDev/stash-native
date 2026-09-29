//! Dedicated Stash vocabulary: no Plex identifiers cross this boundary.
use crate::stash::{Config, Scene};
use crate::stores::stash::Work;
pub use crate::stores::stash::{Action, StashArg, StashMsg};
use crate::ui::machine::*;
use crate::ui::screen::{Mounter, ReturnState, Screen, ScreenArg};
impl ScreenArg for StashArg {
    fn chrome(&self) -> Chrome {
        Chrome::None
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
    pub player_status: &'a str,
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
        Box::new(crate::screens::stash::StashScreen::new(arg.clone(), entry))
    }
}
