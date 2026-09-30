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
