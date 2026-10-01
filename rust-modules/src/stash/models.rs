use serde::{Deserialize, Deserializer, Serialize};

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Scene {
    pub id: String,
    pub title: Option<String>,
    pub date: Option<String>,
    pub details: Option<String>,
    pub studio: Option<Studio>,
    #[serde(deserialize_with = "null_default")]
    pub o_counter: i64,
    #[serde(deserialize_with = "null_default")]
    pub resume_time: f64,
    #[serde(deserialize_with = "null_default")]
    pub play_duration: f64,
    #[serde(deserialize_with = "null_default")]
    pub play_count: i64,
    pub paths: ScenePaths,
    pub files: Vec<SceneFile>,
    #[serde(rename = "sceneStreams")]
    pub scene_streams: Vec<SceneStream>,
    pub performers: Vec<Performer>,
    pub tags: Vec<Tag>,
    pub scene_markers: Vec<SceneMarker>,
}

/// Marker artwork is still imagery; activating a marker seeks within the current session.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SceneMarker {
    pub id: String,
    pub title: String,
    pub seconds: f64,
    pub end_seconds: Option<f64>,
    pub screenshot: Option<String>,
}

/// The small projection used to count a performer's tags without downloading scene media.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SceneTagSummary {
    pub id: String,
    pub tags: Vec<Tag>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Studio {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ScenePaths {
    pub screenshot: Option<String>,
    pub preview: Option<String>,
    pub stream: Option<String>,
    pub webp: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SceneFile {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub audio_codec: String,
    pub format: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SceneStream {
    pub url: String,
    pub mime_type: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Performer {
    pub id: String,
    pub name: String,
    pub image_path: Option<String>,
    pub favorite: bool,
    #[serde(deserialize_with = "null_default")]
    pub o_counter: i64,
    pub details: Option<String>,
    pub birthdate: Option<String>,
    pub death_date: Option<String>,
    pub hair_color: Option<String>,
    pub tags: Vec<Tag>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub image_path: Option<String>,
    pub favorite: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Gallery {
    pub id: String,
    pub title: Option<String>,
    pub image_count: i64,
    pub cover: Option<Image>,
    pub performers: Vec<Performer>,
    pub tags: Vec<Tag>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Image {
    pub id: String,
    pub title: Option<String>,
    pub paths: ImagePaths,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ImagePaths {
    pub image: Option<String>,
    pub thumbnail: Option<String>,
}

impl Scene {
    pub fn display_title(&self) -> &str {
        self.title
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled scene")
    }
}

impl Gallery {
    pub fn display_title(&self) -> &str {
        self.title
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled gallery")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_metadata_accepts_absent_optional_marker_artwork_and_performer_facts() {
        let scene:Scene=serde_json::from_value(serde_json::json!({"id":"19","scene_markers":[{"id":"2","title":"Marker","seconds":29.4,"end_seconds":null,"screenshot":null}],"performers":[{"id":"1","name":"Fixture","details":null,"birthdate":null,"hair_color":null,"tags":[]}]})).unwrap();
        assert_eq!(scene.scene_markers.len(), 1);
        assert_eq!(scene.scene_markers[0].seconds, 29.4);
        assert!(scene.scene_markers[0].screenshot.is_none());
        assert!(scene.performers[0].birthdate.is_none());
        let list: Scene = serde_json::from_value(serde_json::json!({"id":"19"})).unwrap();
        assert!(list.scene_markers.is_empty());
    }
}
