//! Provider-specific screens composed from the shared native UI system.
mod catalog;
mod detail;
pub(crate) mod home;
pub(crate) mod player;
mod settings;
mod viewer;
pub(crate) use catalog::StashScreen;
pub(crate) use detail::SceneDetailScreen;
pub(crate) use settings::SettingsScreen;
pub(crate) use viewer::ViewerScreen;
