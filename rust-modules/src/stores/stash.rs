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
    Loaded {
        generation: u32,
        result: Result<PageData, String>,
    },
    ShelfLoaded {
        generation: u32,
        result: Result<Section, String>,
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
    pub performer: Option<Performer>,
    /// Calendar date captured by the query worker, so screens never read a wall clock.
    pub reference_date: Option<(i32, u32, u32)>,
    pub shelves_error: Option<String>,
}
pub enum Work {
    PerformerTags {
        performer_id: String,
        page: u32,
        generation: u32,
    },
    PerformerTagShelf {
        performer_id: String,
        tag: Tag,
        generation: u32,
    },
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
                        Work::PerformerTagShelf {
                            performer_id,
                            tag,
                            generation,
                        } => StashMsg::ShelfLoaded {
                            generation,
                            result: if fixtures {
                                let data = fixture(&StashArg::Scenes, &Query::default());
                                Ok(shelf(
                                    &tag.name,
                                    data.sections
                                        .into_iter()
                                        .flat_map(|s| s.tiles)
                                        .take(12)
                                        .collect(),
                                ))
                            } else {
                                Client::new(config.clone()).map_err(err).and_then(|c| {
                                    let q = Query {
                                        performer_id: Some(performer_id),
                                        tag_id: Some(tag.id),
                                        sort: "date".into(),
                                        direction: Direction::Descending,
                                        per_page: 12,
                                        ..Default::default()
                                    };
                                    c.scenes(&q)
                                        .map(|p| {
                                            shelf(
                                                &tag.name,
                                                p.items.into_iter().map(scene).collect(),
                                            )
                                        })
                                        .map_err(err)
                                })
                            },
                        },
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
                                        shelf(&tag.name, p.items.into_iter().map(scene).collect())
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
            title: Some(format!("Scene {id}")),
            date: Some("2026-09-01".into()),
            details: Some("A synthetic scene for checking the shared movie detail layout, navigation, and playback controls.".into()),
            studio: Some(crate::stash::Studio { id: "fixture".into(), name: "Fixture Studio".into() }),
            files: vec![crate::stash::SceneFile { duration: 1560., width: if id % 3 == 0 { 1080 } else { 1920 }, height: if id % 3 == 0 { 1920 } else { 1080 }, video_codec: "h264".into(), audio_codec: "aac".into(), format: "mp4".into() }],
            resume_time: if id == 1 { 120. } else { 0. },
            o_counter: id,
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
                        caption: String::new(),
                        image: scene.paths.screenshot.clone(),
                        preview: None,
                        action: Action::Play(scene.clone(), false),
                    },
                    Tile {
                        identity: "resume".into(),
                        title: "Resume".into(),
                        o_count: None,
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
    Tile {
        identity: format!("scene:{}", s.id),
        title: s
            .title
            .clone()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("Scene {}", s.id)),
        o_count: Some(s.o_counter),
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
        caption: if p.favorite { "★".into() } else { String::new() },
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
        shelf: false,
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

#[cfg(test)]
mod pagination_tests {
    use super::*;
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
