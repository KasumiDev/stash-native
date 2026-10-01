//! Serial off-frame Stash query worker. Replies retain addressee and query generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StashArg {
    Home,
    Performers,
    Scenes,
    Galleries,
    Tags,
    Search,
    Settings,
    Scene(String),
    Performer(String),
    Tag(String),
    Gallery(String),
    Viewer { gallery: String, index: usize },
    Player(String),
}
impl StashArg {
    pub fn label(&self) -> &str {
        match self {
            Self::Home => "Home",
            Self::Performers => "Performers",
            Self::Scenes => "Scenes",
            Self::Galleries => "Galleries",
            Self::Tags => "Tags",
            Self::Search => "Search",
            Self::Settings => "Settings",
            Self::Scene(_) => "Scene",
            Self::Performer(_) => "Performer",
            Self::Tag(_) => "Tag",
            Self::Gallery(_) => "Gallery",
            Self::Viewer { .. } => "Images",
            Self::Player(_) => "Player",
        }
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Open(StashArg),
    Play(Scene, bool),
    Refresh,
    Sort,
    EditSearch,
    Pause,
    AddO,
    Replay,
    SeekTo(f64),
    AudioTrack(i32),
    SubtitleTrack(i32),
}
pub enum StashMsg {
    ShelfPageLoaded {
        scope: ShelfScope,
        id: ShelfId,
        generation: u32,
        page: u32,
        result: Result<ShelfPage, String>,
    },
    Loaded {
        generation: u32,
        result: Result<PageData, String>,
    },
    PerformerTags {
        generation: u32,
        page: u32,
        result: Result<crate::stash::Page<crate::stash::SceneTagSummary>, String>,
    },
    Connected(Result<Config, String>),
}
use crate::stash::{Client, Config, Direction, Gallery, Image, Performer, Query, Scene, Tag};
use crate::ui::machine::Addr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
#[derive(Clone, Debug)]
pub struct Tile {
    pub identity: String,
    pub title: String,
    pub o_count: Option<i64>,
    pub scene_metadata: Option<SceneCardMetadata>,
    pub caption: String,
    pub image: Option<String>,
    pub preview: Option<String>,
    pub action: Action,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneCardMetadata {
    pub duration_seconds: Option<f64>,
    pub rating100: Option<i32>,
    pub aspect_ratio: Option<f32>,
}
impl From<&Scene> for SceneCardMetadata {
    fn from(scene: &Scene) -> Self {
        let file = scene.files.first();
        Self {
            duration_seconds: file
                .map(|f| f.duration)
                .filter(|d| d.is_finite() && *d > 0.),
            rating100: scene.rating100.filter(|r| (0..=100).contains(r)),
            aspect_ratio: file
                .filter(|f| f.width > 0 && f.height > 0)
                .map(|f| f.width as f32 / f.height as f32),
        }
    }
}
pub const SHELF_PAGE_SIZE: u32 = 50;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShelfId {
    Favorites,
    Tag(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShelfScope {
    Home,
    Performer(String),
    Tag(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfSpec {
    pub id: ShelfId,
    pub title: String,
    pub scope: ShelfScope,
}
impl ShelfSpec {
    pub fn query(&self, page: u32) -> Query {
        let mut q = Query {
            page: page.max(1),
            per_page: SHELF_PAGE_SIZE,
            sort: "date".into(),
            direction: Direction::Descending,
            ..Default::default()
        };
        match &self.scope {
            ShelfScope::Home => {}
            ShelfScope::Performer(id) => q.performer_id = Some(id.clone()),
            ShelfScope::Tag(id) => q.tag_id = Some(id.clone()),
        }
        match &self.id {
            ShelfId::Favorites => q.rating100 = Some(100),
            ShelfId::Tag(id) => q.tag_ids.push(id.clone()),
        }
        q
    }
}
#[derive(Clone, Debug)]
pub struct ShelfPage {
    pub count: usize,
    pub has_more: bool,
    pub section: Section,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub shelf_id: Option<ShelfId>,
    pub title: String,
    pub tiles: Vec<Tile>,
    pub portrait: bool,
    /// Explicit shelf role; tag names cannot accidentally turn a shelf into a collection grid.
    pub shelf: bool,
}
#[derive(Clone, Debug, Default)]
pub struct PageData {
    pub title: String,
    pub scenes: Vec<Scene>,
    pub scene: Option<Scene>,
    pub sections: Vec<Section>,
    pub count: usize,
    /// Further catalog results exist after this response page.
    pub has_more: bool,
    pub images: Vec<Image>,
    pub lazy_tags: Vec<Tag>,
    pub shelves: Vec<ShelfSpec>,
    pub performer: Option<Performer>,
    /// Calendar date captured by the query worker, so screens never read a wall clock.
    pub reference_date: Option<(i32, u32, u32)>,
    pub shelves_error: Option<String>,
}
pub enum Work {
    ShelfPage {
        spec: ShelfSpec,
        generation: u32,
        page: u32,
    },
    PerformerTags {
        performer_id: String,
        page: u32,
        generation: u32,
    },
    Load {
        route: StashArg,
        query: Query,
        generation: u32,
    },
    Connect(Config),
}
pub struct Worker {
    tx: Sender<(Addr, Work)>,
    rx: Receiver<(Addr, StashMsg)>,
}
impl Worker {
    pub fn new(config: Config, path: PathBuf) -> Self {
        let fixtures =
            cfg!(feature = "hostsim") && std::env::var("STASH_FIXTURES").as_deref() == Ok("1");
        let (tx, jobs) = mpsc::channel();
        let (reply, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("stash-query".into())
            .spawn(move || {
                let mut config = config;
                while let Ok((addr, job)) = jobs.recv() {
                    let msg = match job {
                        Work::ShelfPage {
                            spec,
                            generation,
                            page,
                        } => {
                            let result = if fixtures {
                                Ok(fixture_shelf(&spec, page))
                            } else {
                                Client::new(config.clone()).map_err(err).and_then(|c| {
                                    let q = spec.query(page);
                                    c.scenes(&q)
                                        .map(|p| ShelfPage {
                                            count: p.count,
                                            has_more: page_has_more(&q, p.count),
                                            section: identified_shelf(
                                                &spec,
                                                p.items.into_iter().map(scene).collect(),
                                            ),
                                        })
                                        .map_err(err)
                                })
                            };
                            StashMsg::ShelfPageLoaded {
                                scope: spec.scope,
                                id: spec.id,
                                generation,
                                page,
                                result,
                            }
                        }
                        Work::PerformerTags {
                            performer_id,
                            page,
                            generation,
                        } => StashMsg::PerformerTags {
                            generation,
                            page,
                            result: if fixtures {
                                let q = Query {
                                    page,
                                    per_page: 50,
                                    ..Default::default()
                                };
                                let data = fixture(&StashArg::Scenes, &q);
                                Ok(crate::stash::Page {
                                    count: data.count,
                                    items: data
                                        .scenes
                                        .into_iter()
                                        .map(|s| crate::stash::SceneTagSummary {
                                            id: s.id,
                                            tags: s.tags,
                                        })
                                        .collect(),
                                })
                            } else {
                                Client::new(config.clone()).map_err(err).and_then(|c| {
                                    c.performer_scene_tags(&performer_id, page).map_err(err)
                                })
                            },
                        },
                        Work::Load {
                            route,
                            query,
                            generation,
                        } => StashMsg::Loaded {
                            generation,
                            result: if fixtures {
                                Ok(fixture(&route, &query))
                            } else {
                                Client::new(config.clone())
                                    .map_err(|e| e.to_string())
                                    .and_then(|client| load(&client, &route, &query))
                            },
                        },
                        Work::Connect(next) => {
                            let result = Client::new(next.clone())
                                .and_then(|c| c.test_connection())
                                .map_err(|e| e.to_string())
                                .and_then(|_| next.save(&path).map_err(|e| e.to_string()))
                                .map(|_| {
                                    config = next.clone();
                                    next
                                });
                            StashMsg::Connected(result)
                        }
                    };
                    if reply.send((addr, msg)).is_err() {
                        break;
                    }
                }
            })
            .expect("Stash query worker");
        Self { tx, rx }
    }
    pub fn request(&self, addr: Addr, work: Work) {
        let _ = self.tx.send((addr, work));
    }
    pub fn poll(&self) -> Vec<(Addr, StashMsg)> {
        self.rx.try_iter().collect()
    }
}
/// Synthetic route data for simulator verification; no Stash requests or writes are made.
fn fixture(route: &StashArg, query: &Query) -> PageData {
    let paged = std::env::var_os("STASH_FIXTURE_PAGES").is_some();
    let performers: Vec<_> = (1..=if paged { 108 } else { 8 })
        .map(|id| Performer {
            id: id.to_string(),
            name: format!("Performer {id}"),
            favorite: id < 3,
            o_counter: 100 - id,
            image_path: Some(format!("fixture://performer/{id}")),
            details: Some(
                "A synthetic performer biography for checking portrait and reading layout.".into(),
            ),
            birthdate: Some("1994-08-20".into()),
            hair_color: Some("Brown".into()),
            ..Default::default()
        })
        .collect();
    let tags: Vec<_> = (1..=if paged { 70 } else { 3 })
        .map(|id| Tag {
            id: id.to_string(),
            name: format!("Tag {id}"),
            favorite: true,
            ..Default::default()
        })
        .collect();
    let scenes: Vec<_> = (1..=if paged { 120 } else { 12 })
        .map(|id| Scene {
            id: id.to_string(),
            title: Some(if id == 2 { "A very long synthetic scene title with enough words to verify the first line wraps at half the screen and the second uses more space before the remaining overflow is ellipsized".into() } else { format!("Scene {id}") }),
            date: Some("2026-09-01".into()),
            details: Some("A synthetic scene for checking the shared movie detail layout, navigation, and playback controls.".into()),
            studio: Some(crate::stash::Studio { id: "fixture".into(), name: "Fixture Studio".into() }),
            files: vec![crate::stash::SceneFile { duration: 1560., width: if id % 7 == 0 { 3840 } else if id % 5 == 0 || id % 3 == 0 { 1080 } else { 1920 }, height: if id % 3 == 0 && id % 5 != 0 && id % 7 != 0 { 1920 } else { 1080 }, video_codec: "h264".into(), audio_codec: "aac".into(), format: "mp4".into() }],
            resume_time: if id == 1 { 120. } else { 0. },
            o_counter: id,
            rating100: if id % 4 == 0 { Some(100) } else if id % 7 == 0 { None } else { Some(90) },
            paths: crate::stash::ScenePaths {
                screenshot: Some(format!("fixture://scene/{id}")),
                ..Default::default()
            },
            scene_markers: vec![crate::stash::SceneMarker { id:format!("{id}-1"),title:"Opening".into(),seconds:30.,end_seconds:Some(60.),screenshot:Some(format!("fixture://scene/{id}")) }, crate::stash::SceneMarker { id:format!("{id}-2"), title:"Later scene".into(),seconds:180.,..Default::default() }],
            performers: performers[..2].to_vec(),
            tags: tags.clone(),
            ..Default::default()
        })
        .collect();
    let images: Vec<_> = (1..=if paged { 110 } else { 10 })
        .map(|id| Image {
            id: id.to_string(),
            title: Some(format!("Image {id}")),
            paths: crate::stash::ImagePaths {
                image: Some(format!("fixture://image/{id}")),
                thumbnail: Some(format!("fixture://image/{id}")),
            },
        })
        .collect();
    let galleries: Vec<_> = (1..=if paged { 64 } else { 4 })
        .map(|id| Gallery {
            id: id.to_string(),
            title: Some(format!("Gallery {id}")),
            image_count: 10,
            cover: Some(images[0].clone()),
            ..Default::default()
        })
        .collect();
    let mut data = PageData {
        title: route.label().into(),
        count: 12,
        ..Default::default()
    };
    let shelf_scope = match route {
        StashArg::Home => Some(ShelfScope::Home),
        StashArg::Performer(id) => Some(ShelfScope::Performer(id.clone())),
        StashArg::Tag(id) => Some(ShelfScope::Tag(id.clone())),
        _ => None,
    };
    if let Some(scope) = shelf_scope {
        data.shelves = shelf_specs(scope, &tags);
        data.lazy_tags = tags.clone();
    }
    if matches!(route, StashArg::Scenes | StashArg::Search) {
        data.scenes = scenes.clone();
    }
    match route {
        StashArg::Home => {
            data.scenes = scenes.iter().take(12).cloned().collect();
            data.sections.push(section(
                "Newest scenes",
                data.scenes.iter().cloned().map(scene).collect(),
                false,
            ));
            data.sections.push(section(
                "Performers",
                performers.clone().into_iter().map(performer).collect(),
                true,
            ));
        }
        StashArg::Scenes | StashArg::Search => data.sections.push(section(
            "Scenes",
            scenes
                .into_iter()
                .filter(|s| {
                    s.display_title()
                        .to_lowercase()
                        .contains(&query.q.to_lowercase())
                })
                .map(scene)
                .collect(),
            false,
        )),
        StashArg::Performers => data.sections.push(section(
            "Performers",
            performers.into_iter().map(performer).collect(),
            true,
        )),
        StashArg::Tags => {
            data.sections
                .push(section("Tags", tags.into_iter().map(tag).collect(), false))
        }
        StashArg::Galleries => data.sections.push(section(
            "Galleries",
            galleries.into_iter().map(gallery).collect(),
            false,
        )),
        StashArg::Performer(_) | StashArg::Tag(_) => {
            if let StashArg::Performer(id) = route {
                data.reference_date = Some((2026, 10, 1));
                data.performer = performers.iter().find(|p| &p.id == id).cloned();
                if let Some(p) = &mut data.performer {
                    p.tags = tags.iter().take(2).cloned().collect();
                }
                data.lazy_tags = tags.clone();
            }
            data.scenes = scenes.clone();
            data.sections.push(section(
                "Scenes",
                scenes.into_iter().map(scene).collect(),
                false,
            ));
            data.sections.push(section(
                "Galleries",
                galleries.into_iter().map(gallery).collect(),
                false,
            ));
        }
        StashArg::Scene(id) => {
            let scene = scenes
                .iter()
                .find(|s| &s.id == id)
                .unwrap_or(&scenes[0])
                .clone();
            data.title = scene.display_title().into();
            data.scene = Some(scene.clone());
            data.sections.push(section(
                "Playback",
                vec![
                    Tile {
                        identity: "play".into(),
                        title: "Play from beginning".into(),
                        o_count: None,
                        scene_metadata: None,
                        caption: String::new(),
                        image: scene.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(scene.clone(), false),
                    },
                    Tile {
                        identity: "resume".into(),
                        title: "Resume".into(),
                        o_count: None,
                        scene_metadata: None,
                        caption: String::new(),
                        image: scene.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(scene, true),
                    },
                ],
                false,
            ));
            data.sections.push(section(
                "Performers",
                performers.into_iter().map(performer).collect(),
                true,
            ));
            data.sections
                .push(section("Tags", tags.into_iter().map(tag).collect(), false));
        }
        StashArg::Gallery(id) | StashArg::Viewer { gallery: id, .. } => {
            data.images = images.clone();
            data.count = images.len();
            data.sections.push(section(
                "Images",
                images
                    .into_iter()
                    .enumerate()
                    .map(|(index, image)| Tile {
                        identity: format!("image:{}", image.id),
                        title: image.title.unwrap_or_default(),
                        o_count: None,
                        scene_metadata: None,
                        caption: String::new(),
                        image: image.paths.image,
                        preview: None,
                        action: Action::Open(StashArg::Viewer {
                            gallery: id.clone(),
                            index,
                        }),
                    })
                    .collect(),
                false,
            ));
        }
        StashArg::Settings | StashArg::Player(_) => {}
    }
    if !matches!(
        route,
        StashArg::Scene(_) | StashArg::Viewer { .. } | StashArg::Settings | StashArg::Player(_)
    ) {
        data.count = 0;
        let offset = query.page.saturating_sub(1) as usize * query.per_page as usize;
        for section in &mut data.sections {
            if matches!(route, StashArg::Home) && section.title != "Performers" {
                continue;
            }
            let count = section.tiles.len();
            data.count += count;
            data.has_more |= page_has_more(query, count);
            section.tiles = section
                .tiles
                .iter()
                .skip(offset)
                .take(query.per_page as usize)
                .cloned()
                .collect();
        }
        if matches!(route, StashArg::Gallery(_)) {
            data.images = data
                .images
                .into_iter()
                .skip(offset)
                .take(query.per_page as usize)
                .collect();
        }
        if matches!(
            route,
            StashArg::Scenes | StashArg::Search | StashArg::Performer(_) | StashArg::Tag(_)
        ) {
            data.scenes = data
                .scenes
                .into_iter()
                .skip(offset)
                .take(query.per_page as usize)
                .collect();
        }
    }

    data
}
fn scene(s: Scene) -> Tile {
    let metadata = SceneCardMetadata::from(&s);
    Tile {
        identity: format!("scene:{}", s.id),
        title: s
            .title
            .clone()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("Scene {}", s.id)),
        o_count: Some(s.o_counter),
        scene_metadata: Some(metadata),
        caption: s.date.unwrap_or_else(|| "Undated".into()),
        image: s.paths.screenshot.clone(),
        preview: s.paths.preview.clone(),
        action: Action::Open(StashArg::Scene(s.id)),
    }
}
fn performer(p: Performer) -> Tile {
    Tile {
        identity: format!("performer:{}", p.id),
        title: p.name,
        o_count: Some(p.o_counter),
        scene_metadata: None,
        caption: if p.favorite {
            "★".into()
        } else {
            String::new()
        },
        image: p.image_path,
        preview: None,
        action: Action::Open(StashArg::Performer(p.id)),
    }
}
fn gallery(g: Gallery) -> Tile {
    Tile {
        identity: format!("gallery:{}", g.id),
        title: g.title.unwrap_or_else(|| format!("Gallery {}", g.id)),
        o_count: None,
        scene_metadata: None,
        caption: format!("{} images", g.image_count),
        image: g.cover.and_then(|i| i.paths.thumbnail.or(i.paths.image)),
        preview: None,
        action: Action::Open(StashArg::Gallery(g.id)),
    }
}
fn tag(t: Tag) -> Tile {
    Tile {
        identity: format!("tag:{}", t.id),
        title: t.name,
        o_count: None,
        scene_metadata: None,
        caption: if t.favorite {
            "★".into()
        } else {
            String::new()
        },
        image: t.image_path,
        preview: None,
        action: Action::Open(StashArg::Tag(t.id)),
    }
}
fn section(title: &str, tiles: Vec<Tile>, portrait: bool) -> Section {
    Section {
        shelf_id: None,
        title: title.into(),
        tiles,
        portrait,
        shelf: false,
    }
}
fn identified_shelf(spec: &ShelfSpec, tiles: Vec<Tile>) -> Section {
    Section {
        shelf_id: Some(spec.id.clone()),
        ..shelf(&spec.title, tiles)
    }
}
fn shelf(title: &str, tiles: Vec<Tile>) -> Section {
    Section {
        shelf: true,
        ..section(title, tiles, false)
    }
}
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
fn page_has_more(query: &Query, count: usize) -> bool {
    (query.page.max(1) as usize).saturating_mul(query.per_page as usize) < count
}

fn load(c: &Client, r: &StashArg, q: &Query) -> Result<PageData, String> {
    let mut out = PageData {
        title: r.label().into(),
        ..Default::default()
    };
    match r {
        StashArg::Home => {
            let newest = Query {
                per_page: 12,
                sort: "date".into(),
                direction: Direction::Descending,
                ..Default::default()
            };
            out.scenes = c.scenes(&newest).map_err(err)?.items;
            out.sections.push(section(
                "Newest scenes",
                out.scenes.iter().cloned().map(scene).collect(),
                false,
            ));
            let favorites = Query {
                page: q.page,
                per_page: 50,
                sort: "o_counter".into(),
                direction: Direction::Descending,
                ..Default::default()
            };
            let people = c.favorite_performers(&favorites).map_err(err)?;
            out.count = people.count;
            out.has_more = page_has_more(&favorites, people.count);
            out.sections.push(section(
                "Performers",
                people.items.into_iter().map(performer).collect(),
                true,
            ));
            if q.page > 1 {
                return Ok(out);
            }
            match favorite_tags(c) {
                Ok(tags) => out.lazy_tags = tags,
                Err(error) => out.shelves_error = Some(error),
            }
        }
        StashArg::Scenes | StashArg::Search => {
            let page = c.scenes(q).map_err(err)?;
            out.count = page.count;
            out.has_more = page_has_more(q, page.count);
            out.sections.push(section(
                "Scenes",
                page.items.into_iter().map(scene).collect(),
                false,
            ));
        }
        StashArg::Performers => {
            let page = c.favorite_performers(q).map_err(err)?;
            out.count = page.count;
            out.has_more = page_has_more(q, page.count);
            out.sections.push(section(
                "Performers",
                page.items.into_iter().map(performer).collect(),
                true,
            ));
        }
        StashArg::Galleries => {
            let page = c.galleries(q).map_err(err)?;
            out.count = page.count;
            out.has_more = page_has_more(q, page.count);
            out.sections.push(section(
                "Galleries",
                page.items.into_iter().map(gallery).collect(),
                false,
            ));
        }
        StashArg::Tags => {
            let page = c.tags(q, None).map_err(err)?;
            out.count = page.count;
            out.has_more = page_has_more(q, page.count);
            out.sections.push(section(
                "Tags",
                page.items.into_iter().map(tag).collect(),
                false,
            ));
        }
        StashArg::Scene(id) => {
            let s = c.scene(id).map_err(err)?;
            out.scene = Some(s.clone());
            out.title = s.title.clone().unwrap_or_else(|| "Scene".into());
            out.sections.push(section(
                "Playback",
                vec![
                    Tile {
                        identity: "play".into(),
                        title: "Play from beginning".into(),
                        o_count: None,
                        scene_metadata: None,
                        caption: String::new(),
                        image: s.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(s.clone(), false),
                    },
                    Tile {
                        identity: "resume".into(),
                        title: format!(
                            "Resume at {:.0}:{:02.0}",
                            (s.resume_time / 60.).floor(),
                            s.resume_time % 60.
                        ),
                        o_count: None,
                        scene_metadata: None,
                        caption: String::new(),
                        image: s.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(s.clone(), true),
                    },
                ],
                false,
            ));
            out.sections.push(section(
                "Performers",
                s.performers.into_iter().map(performer).collect(),
                true,
            ));
            out.sections.push(section(
                "Tags",
                s.tags.into_iter().map(tag).collect(),
                false,
            ));
        }
        StashArg::Performer(id) | StashArg::Tag(id) => {
            let mut query = q.clone();
            if matches!(r, StashArg::Performer(_)) {
                let person = c.performer(id).map_err(err)?;
                out.reference_date = Some(c.reference_date());
                out.title = person.name.clone();
                out.performer = Some(person);
                query.performer_id = Some(id.clone());
                if q.page == 1 {
                    match favorite_tags(c) {
                        Ok(tags) => out.lazy_tags = tags,
                        Err(error) => out.shelves_error = Some(error),
                    }
                }
            } else {
                out.title = c.tag(id).map_err(err)?.name;
                query.tag_id = Some(id.clone());
                if q.page == 1 {
                    match favorite_tags(c) {
                        Ok(tags) => out.lazy_tags = tags,
                        Err(error) => out.shelves_error = Some(error),
                    }
                }
            }
            let scenes = c.scenes(&query).map_err(err)?;
            let galleries = c.galleries(&query).map_err(err)?;
            out.count = scenes.count + galleries.count;
            out.has_more = page_has_more(q, scenes.count) || page_has_more(q, galleries.count);
            out.sections.push(section(
                "Scenes",
                scenes.items.into_iter().map(scene).collect(),
                false,
            ));
            out.sections.push(section(
                "Galleries",
                galleries.items.into_iter().map(gallery).collect(),
                false,
            ));
        }
        StashArg::Gallery(id) | StashArg::Viewer { gallery: id, .. } => {
            let mut page = c.images_for_gallery(id, q).map_err(err)?;
            if matches!(r, StashArg::Viewer { .. }) {
                let mut next = q.clone();
                while page.items.len() < page.count {
                    next.page = next.page.saturating_add(1);
                    let more = c.images_for_gallery(id, &next).map_err(err)?;
                    if more.items.is_empty() {
                        break;
                    }
                    page.items.extend(more.items);
                }
            }
            out.count = page.count;
            out.has_more = page_has_more(q, page.count);
            out.images = page.items.clone();
            let tiles = page
                .items
                .into_iter()
                .enumerate()
                .map(|(index, i)| Tile {
                    identity: format!("image:{}", i.id),
                    title: i.title.unwrap_or_else(|| format!("Image {}", index + 1)),
                    o_count: None,
                    scene_metadata: None,
                    caption: String::new(),
                    image: i.paths.thumbnail.or(i.paths.image),
                    preview: None,
                    action: Action::Open(StashArg::Viewer {
                        gallery: id.clone(),
                        index: index + q.page.saturating_sub(1) as usize * q.per_page as usize,
                    }),
                })
                .collect();
            out.sections.push(section("Images", tiles, false));
        }
        StashArg::Settings | StashArg::Player(_) => {}
    }
    if q.page == 1 {
        let scope = match r {
            StashArg::Home => Some(ShelfScope::Home),
            StashArg::Performer(id) => Some(ShelfScope::Performer(id.clone())),
            StashArg::Tag(id) => Some(ShelfScope::Tag(id.clone())),
            _ => None,
        };
        if let Some(scope) = scope {
            out.shelves = shelf_specs(scope, &out.lazy_tags);
        }
    }
    Ok(out)
}

fn favorite_tags(c: &Client) -> Result<Vec<Tag>, String> {
    let mut query = Query {
        per_page: 50,
        sort: "name".into(),
        direction: Direction::Ascending,
        ..Default::default()
    };
    let mut tags = Vec::new();
    loop {
        let page = c.tags(&query, Some(true)).map_err(err)?;
        let count = page.count;
        if page.items.is_empty() {
            break;
        }
        tags.extend(page.items);
        if tags.len() >= count {
            break;
        }
        query.page = query.page.saturating_add(1);
    }
    tags.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    tags.dedup_by(|a, b| a.id == b.id);
    Ok(tags)
}

/// Stable shelf IDs and filters are data-layer facts, independent of display names.
pub fn shelf_specs(scope: ShelfScope, tags: &[Tag]) -> Vec<ShelfSpec> {
    let mut favorite: Vec<_> = tags
        .iter()
        .filter(|t| t.favorite)
        .filter(|t| !matches!(&scope,ShelfScope::Tag(id) if id == &t.id))
        .collect();
    favorite.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut seen = std::collections::HashSet::new();
    favorite.retain(|t| seen.insert(t.id.clone()));
    let mut shelves = Vec::new();
    if !matches!(scope, ShelfScope::Home) {
        shelves.push(ShelfSpec {
            id: ShelfId::Favorites,
            title: "Favorites".into(),
            scope: scope.clone(),
        });
    }
    shelves.extend(favorite.into_iter().map(|t| ShelfSpec {
        id: ShelfId::Tag(t.id.clone()),
        title: t.name.clone(),
        scope: scope.clone(),
    }));
    shelves
}

pub fn fixture_shelf(spec: &ShelfSpec, page: u32) -> ShelfPage {
    // A deterministic fixture exceeding one page; favorites include rated-100 scenes only.
    let source: Vec<_> = (1..=123)
        .map(|id| Scene {
            id: id.to_string(),
            title: Some(format!("Scene {id}")),
            date: if id > 120 {
                None
            } else {
                Some(format!("2026-09-{:02}", 28 - (id % 28)))
            },
            rating100: if id % 3 == 0 {
                None
            } else {
                Some(if id % 2 == 0 { 100 } else { 80 })
            },
            performers: vec![Performer {
                id: "1".into(),
                name: "Fixture performer".into(),
                ..Default::default()
            }],
            tags: vec![
                Tag {
                    id: "1".into(),
                    name: "Alpha".into(),
                    favorite: true,
                    ..Default::default()
                },
                Tag {
                    id: if id % 2 == 0 { "2" } else { "3" }.into(),
                    name: "Other".into(),
                    favorite: true,
                    ..Default::default()
                },
            ],
            files: vec![crate::stash::SceneFile {
                duration: 90. + id as f64,
                width: 1920,
                height: 1080,
                ..Default::default()
            }],
            paths: crate::stash::ScenePaths {
                screenshot: Some(format!("fixture://scene/{id}")),
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
    fixture_shelf_from(spec, page, source)
}

fn fixture_shelf_from(spec: &ShelfSpec, page: u32, source: Vec<Scene>) -> ShelfPage {
    let q = spec.query(page);
    let mut tags = q.tag_ids.clone();
    tags.extend(q.tag_id.iter().cloned());
    let mut seen = std::collections::HashSet::new();
    let mut scenes: Vec<_> = source
        .into_iter()
        .filter(|s| {
            q.rating100.is_none_or(|r| s.rating100 == Some(r))
                && q.performer_id
                    .as_ref()
                    .is_none_or(|id| s.performers.iter().any(|p| &p.id == id))
                && tags.iter().all(|id| s.tags.iter().any(|t| &t.id == id))
                && seen.insert(s.id.clone())
        })
        .collect();
    scenes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
    let count = scenes.len();
    let offset = (q.page - 1) as usize * SHELF_PAGE_SIZE as usize;
    let tiles = scenes
        .into_iter()
        .skip(offset)
        .take(SHELF_PAGE_SIZE as usize)
        .map(scene)
        .collect();
    ShelfPage {
        count,
        has_more: page_has_more(&q, count),
        section: identified_shelf(spec, tiles),
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    #[test]
    fn shelf_pages_preserve_identity_and_continue_after_fifty() {
        let spec = ShelfSpec {
            id: ShelfId::Tag("1".into()),
            title: "Alpha".into(),
            scope: ShelfScope::Home,
        };
        let first = fixture_shelf(&spec, 1);
        let second = fixture_shelf(&spec, 2);
        let last = fixture_shelf(&spec, 3);
        assert_eq!(
            (first.count, first.section.tiles.len(), first.has_more),
            (123, 50, true)
        );
        assert_eq!((second.section.tiles.len(), second.has_more), (50, true));
        assert_eq!((last.section.tiles.len(), last.has_more), (23, false));
        assert_eq!(first.section.shelf_id, Some(spec.id));
        let mut ids = std::collections::HashSet::new();
        assert!(first
            .section
            .tiles
            .iter()
            .chain(&second.section.tiles)
            .chain(&last.section.tiles)
            .all(|t| ids.insert(&t.identity)));
        assert!(last
            .section
            .tiles
            .iter()
            .rev()
            .take(3)
            .all(|t| t.caption == "Undated"));
    }
    #[test]
    fn tag_shelves_use_intersection_and_favorites_are_rating_only() {
        let spec = ShelfSpec {
            id: ShelfId::Tag("2".into()),
            title: "Other".into(),
            scope: ShelfScope::Tag("1".into()),
        };
        let page = fixture_shelf(&spec, 1);
        assert_eq!(page.count, 61);
        let favorites = ShelfSpec {
            id: ShelfId::Favorites,
            title: "Favorites".into(),
            scope: ShelfScope::Performer("1".into()),
        };
        let page = fixture_shelf(&favorites, 1);
        assert!(page
            .section
            .tiles
            .iter()
            .all(|t| t.scene_metadata.as_ref().unwrap().rating100 == Some(100)));
        assert_eq!(page.count, 41);
        let empty = ShelfSpec {
            scope: ShelfScope::Performer("missing".into()),
            ..favorites
        };
        assert_eq!(fixture_shelf(&empty, 1).count, 0);
    }
    #[test]
    fn stable_specs_exclude_viewed_tag_and_sort_favorites_alphabetically() {
        let tags = vec![
            Tag {
                id: "self".into(),
                name: "A".into(),
                favorite: true,
                ..Default::default()
            },
            Tag {
                id: "z".into(),
                name: "Zulu".into(),
                favorite: true,
                ..Default::default()
            },
            Tag {
                id: "b".into(),
                name: "Beta".into(),
                favorite: true,
                ..Default::default()
            },
            Tag {
                id: "hidden".into(),
                name: "Hidden".into(),
                favorite: false,
                ..Default::default()
            },
        ];
        let specs = shelf_specs(ShelfScope::Tag("self".into()), &tags);
        assert_eq!(
            specs.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            vec![
                ShelfId::Favorites,
                ShelfId::Tag("b".into()),
                ShelfId::Tag("z".into())
            ]
        );
        let q = specs[1].query(2);
        assert_eq!(q.per_page, 50);
        assert_eq!(q.tag_id.as_deref(), Some("self"));
        assert_eq!(q.tag_ids, vec!["b"]);
    }
    #[test]
    fn missing_rating_and_invalid_media_dimensions_are_not_fabricated() {
        let scene = Scene {
            files: vec![crate::stash::SceneFile {
                duration: f64::NAN,
                width: 1920,
                height: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            SceneCardMetadata::from(&scene),
            SceneCardMetadata::default()
        );
        let spec = ShelfSpec {
            id: ShelfId::Favorites,
            title: "Favorites".into(),
            scope: ShelfScope::Home,
        };
        assert_eq!(fixture_shelf_from(&spec, 1, vec![scene]).count, 0);
    }
    #[test]
    fn reference_date_is_a_replayable_worker_fact() {
        assert_eq!(
            fixture(&StashArg::Performer("1".into()), &Query::default()).reference_date,
            Some((2026, 10, 1))
        );
    }
    #[test]
    fn page_boundary_and_last_partial_page_are_terminal() {
        let query = Query {
            page: 1,
            per_page: 50,
            ..Default::default()
        };
        assert!(!page_has_more(&query, 50));
        assert!(page_has_more(&query, 51));
        assert!(!page_has_more(&Query { page: 2, ..query }, 51));
    }
}
