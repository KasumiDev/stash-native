use super::{Config, Gallery, Image, Performer, Scene, Tag};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Configuration,
    Storage,
    Transport,
    TimedOut,
    Http(u16),
    Graphql,
    InvalidResponse,
    NotFound,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Enter a valid server URL and API key",
            Self::Storage => "Unable to save or read connection settings",
            Self::Transport => "Unable to reach Stash",
            Self::TimedOut => "Stash request timed out; changes may already have been applied",
            Self::Http(401 | 403) => "Stash refused access; check the API key",
            Self::Http(_) => "Stash returned an HTTP error",
            Self::Graphql => "Stash could not complete the query",
            Self::InvalidResponse => "Stash returned an invalid response",
            Self::NotFound => "This item no longer exists",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    #[default]
    Descending,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub q: String,
    pub page: u32,
    pub per_page: u32,
    pub sort: String,
    pub direction: Direction,
    pub performer_id: Option<String>,
    pub tag_id: Option<String>,
    /// All tags must match, including the singular route tag when present.
    pub tag_ids: Vec<String>,
    pub rating100: Option<i32>,
    pub gallery_id: Option<String>,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            q: String::new(),
            page: 1,
            per_page: 40,
            sort: "date".into(),
            direction: Direction::Descending,
            performer_id: None,
            tag_id: None,
            tag_ids: Vec::new(),
            rating100: None,
            gallery_id: None,
        }
    }
}
impl Query {
    fn filter(&self) -> Value {
        json!({"q": self.q, "page": self.page.max(1), "per_page": self.per_page.clamp(1, 120),
            "sort": self.sort, "direction": if self.direction == Direction::Ascending {"ASC"} else {"DESC"}})
    }
    fn relations(&self) -> Value {
        let mut v = json!({});
        for (key, id) in [
            ("performers", &self.performer_id),
            ("galleries", &self.gallery_id),
        ] {
            if let Some(id) = id {
                v[key] = json!({"value":[id],"modifier":"INCLUDES_ALL"});
            }
        }
        let mut tags = self.tag_ids.clone();
        tags.extend(self.tag_id.iter().cloned());
        tags.sort();
        tags.dedup();
        if !tags.is_empty() {
            v["tags"] = json!({"value":tags,"modifier":"INCLUDES_ALL"});
        }
        if let Some(rating) = self.rating100 {
            v["rating100"] = json!({"value":rating,"modifier":"EQUALS"});
        }
        v
    }
}

#[derive(Clone, Debug)]
pub struct Page<T> {
    pub count: usize,
    pub items: Vec<T>,
}

#[derive(Clone)]
pub struct Client {
    config: Config,
    endpoint: String,
}

const PERFORMER: &str = "id name image_path favorite o_counter";
const TAG: &str = "id name image_path favorite";
const IMAGE: &str = "id title paths { image thumbnail }";
const SCENE: &str = "id title date rating100 details studio { id name } o_counter resume_time play_duration play_count paths { screenshot preview stream webp } files { duration frame_rate width height video_codec audio_codec format } sceneStreams { url mime_type label } performers { id name image_path favorite o_counter } tags { id name image_path favorite }";
const GALLERY: &str = "id title image_count cover { id title paths { image thumbnail } } performers { id name image_path favorite o_counter } tags { id name image_path favorite }";

impl Client {
    /// UTC metadata reference captured by the off-frame adapter, delivered as a fact to screens.
    pub(crate) fn reference_date(&self) -> (i32, u32, u32) {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        date_from_epoch(seconds)
    }
    pub fn new(config: Config) -> Result<Self, Error> {
        let endpoint = config.endpoint()?;
        Ok(Self { config, endpoint })
    }
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Media consumers cannot send the GraphQL ApiKey header. Stash accepts an `apikey` query
    /// parameter; attach it only to the configured authority, never third-party media.
    pub fn media_url(&self, path: &str) -> Result<String, Error> {
        let mut url = self.config.asset_url(path)?;
        let origin = |v: &str| {
            v.split_once("://").map(|(scheme, rest)| {
                (
                    scheme.to_ascii_lowercase(),
                    rest.split('/')
                        .next()
                        .unwrap_or_default()
                        .to_ascii_lowercase(),
                )
            })
        };
        if self.config.api_key.is_empty() || origin(&url) != origin(&self.endpoint) {
            return Ok(url);
        }
        if url
            .split_once('?')
            .is_some_and(|(_, q)| q.split('&').any(|p| p.split('=').next() == Some("apikey")))
        {
            return Ok(url);
        }
        let mut encoded = String::new();
        for byte in self.config.api_key.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                encoded.push(byte as char);
            } else {
                use std::fmt::Write;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str("apikey=");
        url.push_str(&encoded);
        Ok(url)
    }

    /// One attempt only. Additive mutations must never be retried after ambiguous failure.
    pub fn execute(&self, query: &str, variables: Value) -> Result<Value, Error> {
        self.execute_using(query, variables, |endpoint, headers, body| {
            crate::net::request_result(
                endpoint,
                headers,
                "POST",
                Some(body),
                crate::net::API,
                false,
                Some(8 * 1024 * 1024),
                None,
            )
            .map_err(|e| match e {
                crate::net::RequestError::TimedOut => Error::TimedOut,
                crate::net::RequestError::Transport => Error::Transport,
            })
        })
    }

    fn execute_using<F>(&self, query: &str, variables: Value, transport: F) -> Result<Value, Error>
    where
        F: FnOnce(&str, &[String], &[u8]) -> Result<crate::net::Resp, Error>,
    {
        let body = serde_json::to_vec(&json!({"query":query,"variables":variables}))
            .map_err(|_| Error::InvalidResponse)?;
        let mut headers = vec![
            "Content-Type: application/json".to_owned(),
            "Accept: application/json".to_owned(),
        ];
        if !self.config.api_key.is_empty() {
            headers.push(format!("ApiKey: {}", self.config.api_key));
        }
        let response = transport(&self.endpoint, &headers, &body)?;
        decode_response(response.status, &response.body)
    }

    pub fn test_connection(&self) -> Result<(), Error> {
        self.execute("query { version { version } }", json!({}))
            .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)] // GraphQL operation and its typed input/selection names.
    fn list<T: DeserializeOwned>(
        &self,
        operation: &str,
        input: &str,
        argument: &str,
        field: &str,
        selection: &str,
        q: &Query,
        relations: Value,
    ) -> Result<Page<T>, Error> {
        let query = format!("query($filter: FindFilterType, $relations: {input}) {{ {operation}(filter: $filter, {argument}: $relations) {{ count {field} {{ {selection} }} }} }}");
        let result = self.execute(&query, json!({"filter":q.filter(),"relations":relations}))?;
        let node = result.get(operation).ok_or(Error::InvalidResponse)?;
        Ok(Page {
            count: node["count"].as_u64().ok_or(Error::InvalidResponse)? as usize,
            items: serde_json::from_value(node[field].clone())
                .map_err(|_| Error::InvalidResponse)?,
        })
    }

    pub fn scenes(&self, q: &Query) -> Result<Page<Scene>, Error> {
        let mut page: Page<Scene> = self.list(
            "findScenes",
            "SceneFilterType",
            "scene_filter",
            "scenes",
            SCENE,
            q,
            q.relations(),
        )?;
        let mut seen = std::collections::HashSet::new();
        page.items.retain(|scene| seen.insert(scene.id.clone()));
        if q.sort == "date" && q.direction == Direction::Descending {
            page.items
                .sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
        }
        Ok(page)
    }
    pub fn performers(&self, q: &Query, favorite: Option<bool>) -> Result<Page<Performer>, Error> {
        let mut relations = json!({});
        if let Some(favorite) = favorite {
            relations["filter_favorites"] = json!(favorite);
        }
        self.list(
            "findPerformers",
            "PerformerFilterType",
            "performer_filter",
            "performers",
            PERFORMER,
            q,
            relations,
        )
    }
    /// Translate a global page into favorite and nonfavorite pages without interleaving groups.
    pub fn favorite_performers(&self, q: &Query) -> Result<Page<Performer>, Error> {
        favorite_page(q, |q, favorite| {
            stable_performer_page(q, favorite, |q, favorite| {
                self.performers(q, Some(favorite))
            })
        })
    }
    pub fn tags(&self, q: &Query, favorite: Option<bool>) -> Result<Page<Tag>, Error> {
        let relations = favorite.map(|f| json!({"favorite":f})).unwrap_or(json!({}));
        self.list(
            "findTags",
            "TagFilterType",
            "tag_filter",
            "tags",
            TAG,
            q,
            relations,
        )
    }
    pub fn galleries(&self, q: &Query) -> Result<Page<Gallery>, Error> {
        self.list(
            "findGalleries",
            "GalleryFilterType",
            "gallery_filter",
            "galleries",
            GALLERY,
            q,
            q.relations(),
        )
    }
    pub fn images_for_gallery(&self, id: &str, q: &Query) -> Result<Page<Image>, Error> {
        let mut q = q.clone();
        q.gallery_id = Some(id.to_owned());
        self.list(
            "findImages",
            "ImageFilterType",
            "image_filter",
            "images",
            IMAGE,
            &q,
            q.relations(),
        )
    }
    fn detail<T: DeserializeOwned>(
        &self,
        operation: &str,
        selection: &str,
        id: &str,
    ) -> Result<T, Error> {
        let query = format!("query($id: ID!) {{ {operation}(id: $id) {{ {selection} }} }}");
        let v = self.execute(&query, json!({"id":id}))?;
        let node = v
            .get(operation)
            .filter(|v| !v.is_null())
            .ok_or(Error::NotFound)?;
        serde_json::from_value(node.clone()).map_err(|_| Error::InvalidResponse)
    }
    pub fn scene(&self, id: &str) -> Result<Scene, Error> {
        let mut scene: Scene = self.detail(
            "findScene",
            &format!("{SCENE} scene_markers {{ id title seconds end_seconds screenshot preview: stream }}"),
            id,
        )?;
        self.marker_media(&mut scene)?;
        Ok(scene)
    }
    fn marker_media(&self, scene: &mut Scene) -> Result<(), Error> {
        for marker in &mut scene.scene_markers {
            if marker.id.is_empty() {
                marker.screenshot = None;
                marker.preview = None;
                continue;
            }
            marker.screenshot = marker.screenshot.as_deref()
                .filter(|url| !url.trim().is_empty())
                .map(|url| self.media_url(url)).transpose()?;
            marker.preview = marker.preview.as_deref()
                .filter(|url| !url.trim().is_empty())
                .map(|url| self.media_url(url)).transpose()?;
        }
        Ok(())
    }
    pub fn performer(&self, id: &str) -> Result<Performer, Error> {
        self.detail(
            "findPerformer",
            &format!("{PERFORMER} details birthdate death_date hair_color tags {{ {TAG} }}"),
            id,
        )
    }
    pub fn performer_scene_tags(
        &self,
        performer_id: &str,
        page: u32,
    ) -> Result<Page<super::SceneTagSummary>, Error> {
        let query = Query {
            performer_id: Some(performer_id.into()),
            page,
            per_page: 50,
            sort: "id".into(),
            direction: Direction::Ascending,
            ..Default::default()
        };
        self.list(
            "findScenes",
            "SceneFilterType",
            "scene_filter",
            "scenes",
            &format!("id tags {{ {TAG} }}"),
            &query,
            query.relations(),
        )
    }
    pub fn tag(&self, id: &str) -> Result<Tag, Error> {
        self.detail("findTag", TAG, id)
    }
    pub fn save_activity(
        &self,
        id: &str,
        resume_time: f64,
        watched_delta: f64,
    ) -> Result<(), Error> {
        if !resume_time.is_finite()
            || resume_time < 0.0
            || !watched_delta.is_finite()
            || watched_delta < 0.0
        {
            return Err(Error::Configuration);
        }
        self.execute("mutation($id: ID!, $resume: Float!, $duration: Float!) { sceneSaveActivity(id: $id, resume_time: $resume, playDuration: $duration) }",
            json!({"id":id,"resume":resume_time,"duration":watched_delta})).map(|_|())
    }
    pub fn add_play(&self, id: &str) -> Result<(), Error> {
        self.execute(
            "mutation($id: ID!) { sceneAddPlay(id: $id) { count } }",
            json!({"id":id}),
        )
        .map(|_| ())
    }
    pub fn add_o(&self, id: &str) -> Result<i64, Error> {
        let v = self.execute(
            "mutation($id: ID!) { sceneAddO(id: $id) { count } }",
            json!({"id":id}),
        )?;
        v["sceneAddO"]["count"]
            .as_i64()
            .ok_or(Error::InvalidResponse)
    }
}

fn favorite_page<F>(q: &Query, mut fetch_page: F) -> Result<Page<Performer>, Error>
where
    F: FnMut(&Query, bool) -> Result<Page<Performer>, Error>,
{
    let size = q.per_page.clamp(1, 120) as usize;
    let offset = (q.page.max(1) as usize - 1).saturating_mul(size);
    let mut probe = q.clone();
    probe.page = 1;
    probe.per_page = 1;
    probe.sort = "o_counter".into();
    probe.direction = Direction::Descending;
    let favorite_count = fetch_page(&probe, true)?.count;
    let other_count = fetch_page(&probe, false)?.count;
    let mut items = Vec::with_capacity(size);
    for (favorite, count, start) in [
        (true, favorite_count, offset),
        (false, other_count, offset.saturating_sub(favorite_count)),
    ] {
        if (favorite && offset >= favorite_count)
            || (!favorite && offset.saturating_add(size) <= favorite_count)
            || start >= count
        {
            continue;
        }
        let mut fetch = probe.clone();
        fetch.per_page = size as u32;
        fetch.page = (start / size + 1) as u32;
        let skip = start % size;
        let mut group = fetch_page(&fetch, favorite)?.items;
        if skip > 0 {
            group = group.into_iter().skip(skip).collect();
            if group.len() < size - items.len() && start + group.len() < count {
                fetch.page += 1;
                group.extend(fetch_page(&fetch, favorite)?.items);
            }
        }
        items.extend(group.into_iter().take(size - items.len()));
    }
    Ok(Page {
        count: favorite_count + other_count,
        items,
    })
}

/// Stash already sorts counter/name. Normalize identical-name ties, fetching neighboring pages
/// only when a tie straddles a page boundary, so the final ID ordering is pagination-stable.
fn stable_performer_page<F>(
    q: &Query,
    favorite: bool,
    mut fetch: F,
) -> Result<Page<Performer>, Error>
where
    F: FnMut(&Query, bool) -> Result<Page<Performer>, Error>,
{
    let page = fetch(q, favorite)?;
    let count = page.count;
    let size = q.per_page.clamp(1, 120) as usize;
    let same = |a: &Performer, b: &Performer| a.o_counter == b.o_counter && a.name == b.name;
    let mut items = page.items;
    if items.is_empty() {
        return Ok(Page { count, items });
    }
    let first = items[0].clone();
    let last = items.last().unwrap().clone();
    let mut skip = 0;
    let mut previous = q.clone();
    while previous.page > 1 {
        previous.page -= 1;
        let prior = fetch(&previous, favorite)?.items;
        if !prior.last().is_some_and(|v| same(v, &first)) {
            break;
        }
        skip += prior.len();
        items.splice(0..0, prior);
    }
    let mut next = q.clone();
    while (next.page as usize).saturating_mul(size) < count {
        next.page += 1;
        let following = fetch(&next, favorite)?.items;
        if !following.first().is_some_and(|v| same(&last, v)) {
            break;
        }
        items.extend(following);
    }
    let mut begin = 0;
    while begin < items.len() {
        let mut end = begin + 1;
        while end < items.len() && same(&items[begin], &items[end]) {
            end += 1;
        }
        items[begin..end].sort_by(|a, b| a.id.cmp(&b.id));
        begin = end;
    }
    Ok(Page {
        count,
        items: items.into_iter().skip(skip).take(size).collect(),
    })
}

fn decode_response(status: u16, body: &[u8]) -> Result<Value, Error> {
    if !(200..300).contains(&status) {
        return Err(Error::Http(status));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| Error::InvalidResponse)?;
    if value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|v| !v.is_empty())
    {
        return Err(Error::Graphql);
    }
    value
        .get("data")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or(Error::InvalidResponse)
}

fn date_from_epoch(seconds: u64) -> (i32, u32, u32) {
    // Civil date from an epoch-day count, independent of firmware libc struct layouts.
    let z = (seconds / 86400) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    ((y + i64::from(month <= 2)) as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_reference_date_respects_epoch_and_leap_days() {
        assert_eq!(date_from_epoch(0), (1970, 1, 1));
        assert_eq!(date_from_epoch(1_582_934_400), (2020, 2, 29));
        assert_eq!(date_from_epoch(1_790_812_800), (2026, 10, 1));
    }
    #[test]
    fn rejects_partial_graphql_data_and_http_errors() {
        assert_eq!(
            decode_response(
                200,
                br#"{"data":{"findScenes":null},"errors":[{"message":"private"}]}"#
            ),
            Err(Error::Graphql)
        );
        assert_eq!(decode_response(403, b"private"), Err(Error::Http(403)));
        assert_eq!(decode_response(200, b"null"), Err(Error::InvalidResponse));
    }
    #[test]
    fn query_bounds_and_relation_shape() {
        let q = Query {
            page: 0,
            per_page: 999,
            tag_id: Some("tag-id".into()),
            ..Query::default()
        };
        assert_eq!(q.filter()["per_page"], 120);
        assert_eq!(q.filter()["page"], 1);
        assert_eq!(q.relations()["tags"]["modifier"], "INCLUDES_ALL");
    }
    #[test]
    fn scene_rating_and_tag_intersection_are_typed_and_deduplicated() {
        let q = Query {
            rating100: Some(100),
            tag_id: Some("viewed".into()),
            tag_ids: vec!["other".into(), "viewed".into()],
            ..Default::default()
        };
        assert_eq!(
            q.relations()["rating100"],
            json!({"value":100,"modifier":"EQUALS"})
        );
        assert_eq!(
            q.relations()["tags"],
            json!({"value":["other","viewed"],"modifier":"INCLUDES_ALL"})
        );
        let scene: Scene =
            serde_json::from_value(json!({"id":"missing-rating","rating100":null})).unwrap();
        assert_eq!(scene.rating100, None);
    }
    #[test]
    fn scene_decoding_keeps_stash_ids() {
        let scene:Scene=serde_json::from_value(json!({"id":"scene-id","title":null,"date":null,"o_counter":null,"resume_time":null,"play_duration":null,"play_count":null,"sceneStreams":[{"url":"https://example.test/video","mime_type":"video/mp4","label":"Direct"}]})).unwrap();
        assert_eq!(scene.id, "scene-id");
        assert_eq!(scene.scene_streams.len(), 1);
    }
    #[test]
    fn favorites_cross_page_boundary_before_nonfavorites() {
        let source = |q: &Query, favorite: bool| {
            let ids = if favorite {
                vec!["f1", "f2", "f3"]
            } else {
                vec!["n1", "n2", "n3", "n4", "n5"]
            };
            let start = (q.page.max(1) - 1) as usize * q.per_page as usize;
            Ok(Page {
                count: ids.len(),
                items: ids
                    .into_iter()
                    .skip(start)
                    .take(q.per_page as usize)
                    .map(|id| Performer {
                        id: id.into(),
                        favorite,
                        ..Performer::default()
                    })
                    .collect(),
            })
        };
        let q = Query {
            page: 2,
            per_page: 2,
            ..Query::default()
        };
        let page = favorite_page(&q, source).unwrap();
        assert_eq!(
            page.items.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["f3", "n1"]
        );
        let q = Query {
            page: 3,
            per_page: 2,
            ..Query::default()
        };
        let page = favorite_page(&q, source).unwrap();
        assert_eq!(
            page.items.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["n2", "n3"]
        );
    }
    #[test]
    fn exact_counter_and_name_ties_sort_by_id_across_pages() {
        let source = |q: &Query, _favorite: bool| {
            let ids = ["d", "b", "c", "a", "z"];
            let start = (q.page - 1) as usize * q.per_page as usize;
            Ok(Page {
                count: ids.len(),
                items: ids
                    .into_iter()
                    .skip(start)
                    .take(q.per_page as usize)
                    .map(|id| Performer {
                        id: id.into(),
                        name: if id == "z" { "Z".into() } else { "Same".into() },
                        o_counter: 10,
                        ..Performer::default()
                    })
                    .collect(),
            })
        };
        for (page, expected) in [(1, vec!["a", "b"]), (2, vec!["c", "d"]), (3, vec!["z"])] {
            let q = Query {
                page,
                per_page: 2,
                ..Query::default()
            };
            let result = stable_performer_page(&q, true, source).unwrap();
            assert_eq!(
                result
                    .items
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    #[test]
    fn marker_media_preserves_server_scene_path_and_same_origin_auth() {
        let client = Client::new(Config {
            server_url: "https://example.test/graphql".into(),
            api_key: "a& b".into(),
        })
        .unwrap();
        let mut scene = Scene {
            scene_markers: vec![
                super::super::SceneMarker {
                    id: "19/unsafe".into(),
                    screenshot: Some("https://example.test/scene/synthetic/scene_marker/19/screenshot".into()),
                    preview: Some("https://example.test/scene/synthetic/scene_marker/19/stream".into()),
                    ..Default::default()
                },
                super::super::SceneMarker::default(),
            ],
            ..Default::default()
        };
        client.marker_media(&mut scene).unwrap();
        assert_eq!(
            scene.scene_markers[0].screenshot.as_deref(),
            Some("https://example.test/scene/synthetic/scene_marker/19/screenshot?apikey=a%26%20b")
        );
        assert_eq!(
            scene.scene_markers[0].preview.as_deref(),
            Some("https://example.test/scene/synthetic/scene_marker/19/stream?apikey=a%26%20b")
        );
        assert!(scene.scene_markers[1].preview.is_none());
    }
    #[test]
    fn media_auth_never_escapes_configured_origin() {
        let c = Client::new(Config {
            server_url: "https://example.test".into(),
            api_key: "a& b".into(),
        })
        .unwrap();
        assert_eq!(
            c.media_url("/scene/stream").unwrap(),
            "https://example.test/scene/stream?apikey=a%26%20b"
        );
        assert_eq!(
            c.media_url("https://other.test/stream").unwrap(),
            "https://other.test/stream"
        );
        assert!(c.media_url("//other.test/stream").is_err());
    }
    #[test]
    fn mock_graphql_transport_posts_variables_and_surfaces_ambiguous_failure_once() {
        let c = Client::new(Config {
            server_url: "https://example.test/graphql".into(),
            api_key: "synthetic-key".into(),
        })
        .unwrap();
        let result = c.execute_using(
            "mutation($id: ID!){sceneAddO(id:$id){count}}",
            json!({"id":"synthetic-scene"}),
            |url, headers, body| {
                assert_eq!(url, "https://example.test/graphql");
                assert!(headers.contains(&"ApiKey: synthetic-key".to_owned()));
                let body: Value = serde_json::from_slice(body).unwrap();
                assert_eq!(body["variables"]["id"], "synthetic-scene");
                Err(Error::TimedOut)
            },
        );
        assert_eq!(result, Err(Error::TimedOut));
        let result = c
            .execute_using("query { version { version } }", json!({}), |_, _, _| {
                Ok(crate::net::Resp {
                    status: 200,
                    body: br#"{"data":{"version":{"version":"v0.31.1"}}}"#.to_vec(),
                })
            })
            .unwrap();
        assert_eq!(result["version"]["version"], "v0.31.1");
    }
}
