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
    More,
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
    Loaded {
        generation: u32,
        result: Result<PageData, String>,
    },
    ShelfLoaded {
        generation: u32,
        result: Result<Section, String>,
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
    pub caption: String,
    pub image: Option<String>,
    pub preview: Option<String>,
    pub action: Action,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub title: String,
    pub tiles: Vec<Tile>,
    pub portrait: bool,
}
#[derive(Clone, Debug, Default)]
pub struct PageData {
    pub title: String,
    pub scenes: Vec<Scene>,
    pub scene: Option<Scene>,
    pub sections: Vec<Section>,
    pub count: usize,
    pub images: Vec<Image>,
    pub lazy_tags: Vec<Tag>,
}
pub enum Work {
    TagShelf {
        tag: Tag,
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
                        Work::TagShelf { tag, generation } => StashMsg::ShelfLoaded {
                            generation,
                            result: Client::new(config.clone()).map_err(err).and_then(|c| {
                                let q = Query {
                                    tag_id: Some(tag.id),
                                    sort: "date".into(),
                                    direction: Direction::Descending,
                                    per_page: 12,
                                    ..Default::default()
                                };
                                c.scenes(&q)
                                    .map(|p| {
                                        section(
                                            &tag.name,
                                            p.items.into_iter().map(scene).collect(),
                                            false,
                                        )
                                    })
                                    .map_err(err)
                            }),
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
    let performers: Vec<_> = (1..=8)
        .map(|id| Performer {
            id: id.to_string(),
            name: format!("Performer {id}"),
            favorite: id < 3,
            o_counter: 100 - id,
            image_path: Some(format!("fixture://performer/{id}")),
        })
        .collect();
    let tags: Vec<_> = (1..=3)
        .map(|id| Tag {
            id: id.to_string(),
            name: format!("Tag {id}"),
            favorite: true,
            ..Default::default()
        })
        .collect();
    let scenes: Vec<_> = (1..=12)
        .map(|id| Scene {
            id: id.to_string(),
            title: Some(format!("Scene {id}")),
            date: Some("2026-09-01".into()),
            details: Some("A synthetic scene for checking the shared movie detail layout, navigation, and playback controls.".into()),
            studio: Some(crate::stash::Studio { id: "fixture".into(), name: "Fixture Studio".into() }),
            files: vec![crate::stash::SceneFile { duration: 1560., width: 1920, height: 1080, video_codec: "h264".into(), audio_codec: "aac".into(), format: "mp4".into() }],
            resume_time: if id == 1 { 120. } else { 0. },
            o_counter: id,
            paths: crate::stash::ScenePaths {
                screenshot: Some(format!("fixture://scene/{id}")),
                ..Default::default()
            },
            performers: performers[..2].to_vec(),
            tags: tags.clone(),
            ..Default::default()
        })
        .collect();
    let images: Vec<_> = (1..=10)
        .map(|id| Image {
            id: id.to_string(),
            title: Some(format!("Image {id}")),
            paths: crate::stash::ImagePaths {
                image: Some(format!("fixture://image/{id}")),
                thumbnail: Some(format!("fixture://image/{id}")),
            },
        })
        .collect();
    let galleries: Vec<_> = (1..=4)
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
    match route {
        StashArg::Home => {
            data.scenes = scenes.clone();
            data.sections.push(section(
                "Newest scenes",
                scenes.clone().into_iter().map(scene).collect(),
                false,
            ));
            data.sections.push(section(
                "Performers",
                performers.clone().into_iter().map(performer).collect(),
                true,
            ));
            data.sections.push(section(
                "Tag 1",
                scenes.into_iter().map(scene).collect(),
                false,
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
                        caption: String::new(),
                        image: scene.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(scene.clone(), false),
                    },
                    Tile {
                        identity: "resume".into(),
                        title: "Resume".into(),
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
    data
}
fn scene(s: Scene) -> Tile {
    Tile {
        identity: format!("scene:{}", s.id),
        title: s
            .title
            .clone()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("Scene {}", s.id)),
        caption: format!(
            "{} · O {}",
            s.date.as_deref().unwrap_or("Undated"),
            s.o_counter
        ),
        image: s.paths.screenshot.clone(),
        preview: s.paths.preview.clone(),
        action: Action::Open(StashArg::Scene(s.id)),
    }
}
fn performer(p: Performer) -> Tile {
    Tile {
        identity: format!("performer:{}", p.id),
        title: p.name,
        caption: format!("{}O {}", if p.favorite { "★ · " } else { "" }, p.o_counter),
        image: p.image_path,
        preview: None,
        action: Action::Open(StashArg::Performer(p.id)),
    }
}
fn gallery(g: Gallery) -> Tile {
    Tile {
        identity: format!("gallery:{}", g.id),
        title: g.title.unwrap_or_else(|| format!("Gallery {}", g.id)),
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
        title: title.into(),
        tiles,
        portrait,
    }
}
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
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
                per_page: 24,
                sort: "o_counter".into(),
                direction: Direction::Descending,
                ..Default::default()
            };
            out.sections.push(section(
                "Performers",
                c.favorite_performers(&favorites)
                    .map_err(err)?
                    .items
                    .into_iter()
                    .map(performer)
                    .collect(),
                true,
            ));
            let mut tags = c
                .tags(
                    &Query {
                        per_page: 100,
                        sort: "name".into(),
                        ..Default::default()
                    },
                    Some(true),
                )
                .map_err(err)?;
            let mut tag_query = Query {
                per_page: 100,
                sort: "name".into(),
                ..Default::default()
            };
            while tags.items.len() < tags.count {
                tag_query.page = tag_query.page.saturating_add(1);
                let page = c.tags(&tag_query, Some(true)).map_err(err)?;
                if page.items.is_empty() {
                    break;
                }
                tags.items.extend(page.items);
            }
            out.lazy_tags = tags.items;
        }
        StashArg::Scenes | StashArg::Search => {
            let page = c.scenes(q).map_err(err)?;
            out.count = page.count;
            out.sections.push(section(
                "Scenes",
                page.items.into_iter().map(scene).collect(),
                false,
            ));
        }
        StashArg::Performers => {
            let page = c.favorite_performers(q).map_err(err)?;
            out.count = page.count;
            out.sections.push(section(
                "Performers",
                page.items.into_iter().map(performer).collect(),
                true,
            ));
        }
        StashArg::Galleries => {
            let page = c.galleries(q).map_err(err)?;
            out.count = page.count;
            out.sections.push(section(
                "Galleries",
                page.items.into_iter().map(gallery).collect(),
                false,
            ));
        }
        StashArg::Tags => {
            let page = c.tags(q, None).map_err(err)?;
            out.count = page.count;
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
                out.title = c.performer(id).map_err(err)?.name;
                query.performer_id = Some(id.clone());
            } else {
                out.title = c.tag(id).map_err(err)?.name;
                query.tag_id = Some(id.clone());
            }
            out.sections.push(section(
                "Scenes",
                c.scenes(&query)
                    .map_err(err)?
                    .items
                    .into_iter()
                    .map(scene)
                    .collect(),
                false,
            ));
            out.sections.push(section(
                "Galleries",
                c.galleries(&query)
                    .map_err(err)?
                    .items
                    .into_iter()
                    .map(gallery)
                    .collect(),
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
            out.images = page.items.clone();
            let tiles = page
                .items
                .into_iter()
                .enumerate()
                .map(|(index, i)| Tile {
                    identity: format!("image:{}", i.id),
                    title: i.title.unwrap_or_else(|| format!("Image {}", index + 1)),
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
    Ok(out)
}
