//! Real scene identity and Stash history sit beside the existing native player session.
use crate::stash::{Client, HistoryCommand, HistoryTracker, Scene};
use crate::{player, route};
use std::sync::mpsc;
use std::time::Duration;
pub(crate) struct Playback {
    adapter: player::adapter::PlayerAdapter,
    session: route::PlaybackSession,
    history: Option<HistoryTracker>,
    commands: Option<mpsc::SyncSender<Queued>>,
    results: mpsc::Receiver<(u64, bool, Result<Option<i64>, String>)>,
    last_tick: Option<u64>,
    client: Option<Client>,
    generation: u64,
    clock: ActivityClock,
    selected: Option<Selected>,
    resolved_url: Option<String>,
    completed: bool,
    repause_at: Option<i64>,
    o_pending: bool,
    o_last: Option<u64>,
    pub sync_error: Option<String>,
    pub o_count: Option<i64>,
    pub scene_id: Option<String>,
}
enum Queued {
    Mutation {
        generation: u64,
        client: Client,
        command: HistoryCommand,
    },
    Barrier(mpsc::SyncSender<()>),
}
#[derive(Default)]
struct ActivityClock {
    position: f64,
    wall: Duration,
}
fn paused_seek_landed(target: i64, pending: i64, frames: i32, position: i64) -> bool {
    pending < 0 && frames >= 1 && position.saturating_add(15_000_000_000) >= target
}
fn toggle_transport(
    paused: bool,
    mut actuator: impl FnMut(bool) -> bool,
) -> Result<bool, &'static str> {
    let pause = !paused;
    if actuator(pause) {
        Ok(pause)
    } else if pause {
        Err("The player could not pause playback")
    } else {
        Err("The player could not resume playback")
    }
}
impl ActivityClock {
    fn sample(&mut self, elapsed: Duration, position: f64, moving: bool) -> Duration {
        let advance = position - self.position;
        self.position = position;
        if !moving {
            self.wall = Duration::ZERO;
            return Duration::ZERO;
        }
        self.wall = self
            .wall
            .saturating_add(elapsed)
            .min(Duration::from_secs(1));
        if advance <= 0.0 {
            return Duration::ZERO;
        }
        let watched = if advance <= self.wall.as_secs_f64() + 0.3 {
            Duration::from_secs_f64(advance.min(self.wall.as_secs_f64()))
        } else {
            Duration::ZERO
        };
        self.wall = Duration::ZERO;
        watched
    }
}
impl Playback {
    pub fn new(mt: crate::task::MainThread) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Queued>(64);
        let (rtx, results) = mpsc::channel();
        let commands = std::thread::Builder::new()
            .name("stash history".into())
            .spawn(move || {
                while let Ok(queued) = rx.recv() {
                    let (generation, client, command) = match queued {
                        Queued::Mutation {
                            generation,
                            client,
                            command,
                        } => (generation, client, command),
                        Queued::Barrier(done) => {
                            let _ = done.send(());
                            continue;
                        }
                    };
                    let is_o = matches!(command, HistoryCommand::AddO { .. });
                    let result = match command {
                        HistoryCommand::SaveActivity {
                            id,
                            resume_time,
                            watched_delta,
                        } => client
                            .save_activity(&id, resume_time, watched_delta)
                            .map(|_| None),
                        HistoryCommand::AddPlay { id } => client.add_play(&id).map(|_| None),
                        HistoryCommand::AddO { id } => client.add_o(&id).map(Some),
                    }
                    .map_err(|_| "Could not synchronize scene activity".to_owned());
                    let _ = rtx.send((generation, is_o, result));
                }
            })
            .ok()
            .map(|_| tx);
        Self {
            adapter: player::adapter::PlayerAdapter::new(mt),
            session: route::PlaybackSession::default(),
            history: None,
            commands,
            results,
            last_tick: None,
            client: None,
            generation: 0,
            clock: ActivityClock::default(),
            selected: None,
            resolved_url: None,
            completed: false,
            repause_at: None,
            o_pending: false,
            o_last: None,
            sync_error: None,
            o_count: None,
            scene_id: None,
        }
    }
    pub fn start(&mut self, client: Client, scene: &Scene, resume: bool) -> Result<(), String> {
        self.stop();
        player::reset_audio_track();
        player::reset_subtitle();
        if self.commands.is_none() {
            return Err("Could not start activity synchronization".into());
        }
        self.generation = self.generation.wrapping_add(1);
        let stream = select_stream(scene).ok_or("No compatible scene stream is available")?;
        let url = client
            .media_url(&stream.url)
            .map_err(|_| "The stream URL is invalid")?;
        let offset = if resume {
            scene.resume_time.max(0.0)
        } else {
            0.0
        };
        let start_url = if stream.transcoded {
            offset_url(&url, offset)
        } else {
            url.clone()
        };
        if !route::prepare_stash_stream(
            &mut self.session,
            &scene.id,
            &start_url,
            &stream.video,
            &stream.audio,
            stream.width,
            stream.height,
            stream.fps,
        ) {
            return Err("The scene cannot be played by this TV".into());
        }
        if !player::engine::start_bufferfeed(&mut self.session, &mut self.adapter) {
            return Err("The video engine could not start".into());
        }
        if stream.transcoded {
            player::engine::stash_stream_offset((offset * 1e9) as i64);
        } else if offset > 0.0 {
            player::request_seek((offset * 1e9) as i64);
        }
        let mut history = HistoryTracker::new(scene.id.clone());
        let _ = history.tick(Duration::ZERO, offset, false);
        self.history = Some(history);
        self.client = Some(client);
        self.selected = Some(stream);
        self.resolved_url = Some(url);
        self.scene_id = Some(scene.id.clone());
        self.o_count = Some(scene.o_counter);
        self.last_tick = None;
        self.clock = ActivityClock {
            position: offset,
            ..Default::default()
        };
        self.sync_error = None;
        self.completed = false;
        self.o_pending = false;
        self.o_last = None;
        Ok(())
    }
    pub fn tick(&mut self, now_ms: u64) {
        while let Ok((generation, is_o, result)) = self.results.try_recv() {
            if generation != self.generation {
                if let Err(error) = result {
                    self.sync_error = Some(error);
                }
                continue;
            }
            if is_o {
                self.o_pending = false;
                if let Some(history) = &mut self.history {
                    history.o_finished();
                }
            }
            match result {
                Ok(Some(count)) => self.o_count = Some(count),
                Ok(None) => {}
                Err(error) => self.sync_error = Some(error),
            }
        }
        if self.history.is_none() || self.completed {
            return;
        }
        player::pump(&mut self.session, &mut self.adapter, now_ms as u32);
        route::drain_route_start_results(&mut self.session);
        if self.repause_at.is_some_and(|target| {
            paused_seek_landed(
                target,
                player::seek_pending(),
                player::frames(),
                player::playpos_ns(),
            )
        }) && player::seek_preroll_active()
            && player::finish_paused_seek(&mut self.adapter)
        {
            self.repause_at = None;
        }
        let position = player::playpos_ns().max(0) as f64 / 1e9;
        let elapsed = self
            .last_tick
            .replace(now_ms)
            .map(|last| Duration::from_millis(now_ms.saturating_sub(last).min(1000)))
            .unwrap_or_default();
        // Count only a forward, near-real-time native position advance. Frozen buffering clocks,
        // paused frames and large seek jumps must never contribute watched duration.
        let playing = self.playing() && player::seen_frame() && player::seek_pending() < 0;
        let watched = self.clock.sample(elapsed, position, playing);
        let commands = self
            .history
            .as_mut()
            .unwrap()
            .tick(watched, position, playing);
        for command in commands {
            self.send(command);
        }
        if player::ended() {
            if let Some(command) = completion_flush(self.history.as_mut(), &mut self.completed) {
                self.send(command);
                player::engine::stop_bufferfeed(&mut self.session, &mut self.adapter);
                self.last_tick = None;
            }
        }
    }
    pub fn pause(&mut self) {
        if self.completed {
            return;
        }
        if player::pause(&mut self.adapter) {
            self.flush(false);
        } else {
            self.sync_error = Some("The player could not pause playback".into());
        }
    }
    pub fn resume(&mut self) {
        if self.completed {
            return;
        }
        if player::resume(&mut self.adapter) {
            self.repause_at = None;
        } else {
            self.sync_error = Some("The player could not resume playback".into());
        }
    }
    pub fn toggle_pause(&mut self) {
        // Buffering/seeking is not a user pause. Toggle the accepted transport intent,
        // not the derived Playing state, which is false during clock transitions too.
        if self.completed {
            return;
        }
        let paused = player::TX.paused.load(std::sync::atomic::Ordering::Acquire);
        match toggle_transport(paused, |pause| {
            if pause {
                player::pause(&mut self.adapter)
            } else {
                player::resume(&mut self.adapter)
            }
        }) {
            Ok(true) => self.flush(false),
            Ok(false) => self.repause_at = None,
            Err(error) => self.sync_error = Some(error.into()),
        }
    }
    pub fn set_paused(&mut self, paused: bool) {
        if player::TX.paused.load(std::sync::atomic::Ordering::Acquire) == paused {
            return;
        }
        if paused {
            self.pause();
        } else {
            self.resume();
        }
    }
    pub fn seek(&mut self, seconds: f64) {
        if self.completed || !seconds.is_finite() || self.adapter.engine().is_none() {
            self.sync_error = Some("The player could not seek to the selected time".into());
            return;
        }
        let seconds = seconds.max(0.0);
        let was_paused = player::TX.paused.load(std::sync::atomic::Ordering::Acquire);
        if let (Some(stream), Some(url), Some(id)) =
            (&self.selected, &self.resolved_url, &self.scene_id)
        {
            if stream.transcoded {
                let (stream, url, id) = (stream.clone(), offset_url(url, seconds), id.clone());
                self.flush(false);
                player::engine::stop_bufferfeed(&mut self.session, &mut self.adapter);
                if route::prepare_stash_stream(
                    &mut self.session,
                    &id,
                    &url,
                    &stream.video,
                    &stream.audio,
                    stream.width,
                    stream.height,
                    stream.fps,
                ) && player::engine::start_bufferfeed(&mut self.session, &mut self.adapter)
                {
                    player::engine::stash_stream_offset((seconds * 1e9) as i64);
                } else {
                    self.sync_error =
                        Some("Could not restart the scene at the selected time".into());
                    // No new timeline was accepted. Do not publish seek preroll or
                    // reset activity accounting to a target that never started.
                    return;
                }
            } else {
                player::request_seek((seconds * 1e9) as i64);
            }
        } else {
            self.sync_error = Some("The player could not seek to the selected time".into());
            return;
        }
        self.last_tick = None;
        if was_paused {
            // A server-stream restart resets transport state. Restore the viewer's intent
            // before the next pump, with the original bounded one-frame seek preroll.
            player::TX.commit_paused(true);
            player::TX.begin_paused_seek();
            player::TX
                .resume_pend
                .store(true, std::sync::atomic::Ordering::Release);
            self.repause_at = Some((seconds * 1e9) as i64);
        }
        self.clock = ActivityClock {
            position: seconds,
            ..Default::default()
        };
    }
    pub fn o_key(&mut self, down: bool, now_ms: u64) {
        if down
            && (self.o_pending
                || self
                    .o_last
                    .is_some_and(|last| now_ms.saturating_sub(last) < 2000))
        {
            return;
        }
        let command = self.history.as_mut().and_then(|h| h.o_key(down));
        if let Some(command) = command {
            self.o_last = Some(now_ms);
            self.o_pending = true;
            self.send(command);
        }
    }
    pub fn stop(&mut self) {
        self.stop_completed(false);
    }
    /// The app's quit path waits for the serialized final save before terminating workers.
    /// A stalled server remains bounded; ambiguous additive operations are never replayed.
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.generation == 0 {
            return Ok(());
        }
        self.stop();
        let (tx, rx) = mpsc::sync_channel(1);
        if self
            .commands
            .as_ref()
            .is_none_or(|commands| commands.try_send(Queued::Barrier(tx)).is_err())
        {
            return Err("Could not enqueue the final activity synchronization".into());
        }
        if rx.recv_timeout(Duration::from_secs(26)).is_err() {
            return Err("Final activity synchronization did not finish before exit".into());
        }
        self.tick(0);
        self.sync_error.clone().map_or(Ok(()), Err)
    }
    fn stop_completed(&mut self, completed: bool) {
        if self.history.is_some() && !self.completed {
            self.flush(completed);
        }
        player::engine::stop_bufferfeed(&mut self.session, &mut self.adapter);
        self.history = None;
        self.client = None;
        self.scene_id = None;
        self.selected = None;
        self.resolved_url = None;
        self.last_tick = None;
        self.completed = false;
        self.repause_at = None;
        self.o_pending = false;
    }
    fn flush(&mut self, completed: bool) {
        if let Some(history) = &mut self.history {
            let command = history.flush(completed);
            self.send(command);
        }
    }
    fn send(&mut self, command: HistoryCommand) {
        let is_o = matches!(command, HistoryCommand::AddO { .. });
        let failed = match (&self.commands, &self.client) {
            (Some(tx), Some(client)) => tx
                .try_send(Queued::Mutation {
                    generation: self.generation,
                    client: client.clone(),
                    command,
                })
                .is_err(),
            _ => true,
        };
        if failed {
            self.sync_error = Some("Activity queue is full; this update was not replayed".into());
            if is_o {
                self.o_pending = false;
                if let Some(history) = &mut self.history {
                    history.o_finished();
                }
            }
        }
    }
    pub fn position(&self) -> f64 {
        if self.completed {
            return self.duration();
        }
        player::playpos_ns().max(0) as f64 / 1e9
    }
    pub fn duration(&self) -> f64 {
        self.selected
            .as_ref()
            .map(|s| s.duration)
            .filter(|&d| d > 0.0)
            .unwrap_or_else(|| player::duration_ns().max(0) as f64 / 1e9)
    }
    pub fn playing(&self) -> bool {
        !self.completed
            && player::is_playing(&self.session)
            && !player::TX.paused.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn error(&self) -> Option<String> {
        (!self.completed && self.scene_id.is_some() && player::has_error(&self.session))
            .then(|| player::error_reason(&self.session).to_owned())
    }
    pub fn completed(&self) -> bool {
        self.completed
    }
    pub fn loading(&self) -> bool {
        !self.completed && self.scene_id.is_some() && player::loading(&self.session)
    }
    pub fn o_pending(&self) -> bool {
        self.o_pending
    }
    pub fn select_audio(&mut self, ordinal: i32) {
        let codec = player::SHARED
            .track_names
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .audio_codecs
            .get(ordinal.max(0) as usize)
            .cloned();
        if let Some(codec) = codec.filter(|_| ordinal >= 0) {
            player::request_audio_track(&mut self.session, ordinal, &codec);
        }
    }
    pub fn subtitle_track(&mut self, ordinal: i32) {
        player::request_subtitle(ordinal);
    }
}
fn completion_flush(
    history: Option<&mut HistoryTracker>,
    completed: &mut bool,
) -> Option<HistoryCommand> {
    if *completed {
        return None;
    }
    let command = history?.flush(true);
    *completed = true;
    Some(command)
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}
#[derive(Clone)]
struct Selected {
    url: String,
    video: String,
    audio: String,
    width: u16,
    height: u16,
    fps: f64,
    transcoded: bool,
    duration: f64,
}
fn offset_url(url: &str, seconds: f64) -> String {
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut fields: Vec<_> = query
        .split('&')
        .filter(|s| !s.is_empty() && !s.starts_with("start="))
        .map(str::to_owned)
        .collect();
    fields.push(format!("start={seconds:.3}"));
    format!("{base}?{}", fields.join("&"))
}
fn select_stream(scene: &Scene) -> Option<Selected> {
    let file = scene.files.first();
    let mut video = file
        .map(|f| f.video_codec.to_ascii_lowercase())
        .unwrap_or_default();
    let audio = file
        .map(|f| f.audio_codec.to_ascii_lowercase())
        .unwrap_or_default();
    if video == "h265" {
        video = "hevc".into();
    }
    if video == "avc" {
        video = "h264".into();
    }
    let direct = matches!(video.as_str(), "h264" | "hevc")
        && matches!(audio.as_str(), "aac" | "ac3" | "eac3" | "dts" | "");
    if direct {
        if let Some(url) = &scene.paths.stream {
            return Some(Selected {
                url: url.clone(),
                video,
                audio,
                width: file.unwrap().width.min(u16::MAX as u32) as u16,
                height: file.unwrap().height.min(u16::MAX as u32) as u16,
                fps: source_fps(file.unwrap().frame_rate),
                transcoded: false,
                duration: file.unwrap().duration,
            });
        }
    }
    // Only progressive MP4/MKV is accepted: the bundled demuxer has no HLS network protocol.
    // Stash v0.31.1 pkg/ffmpeg/stream_transcode.go selects H264 for MP4, with the MP4
    // muxer's AAC default and two audio channels. MKV uses Opus, which this pipeline rejects.
    scene
        .scene_streams
        .iter()
        .filter(|s| {
            s.mime_type.as_deref() == Some("video/mp4")
                && s.url
                    .split('?')
                    .next()
                    .is_some_and(|p| p.ends_with("/stream.mp4"))
        })
        .min_by_key(|s| {
            if s.label.as_deref().is_some_and(|l| l.contains("720p")) {
                0
            } else {
                1
            }
        })
        .map(|s| Selected {
            url: s.url.clone(),
            video: "h264".into(),
            audio: if audio.is_empty() {
                String::new()
            } else {
                "aac".into()
            },
            width: 1280,
            height: 720,
            // Stash does not declare the encoded stream's rate; never claim that
            // its source metadata necessarily describes the server output.
            fps: 0.0,
            transcoded: true,
            duration: file.map(|f| f.duration).unwrap_or_default(),
        })
}
fn source_fps(fps: f64) -> f64 {
    if fps.is_finite() && fps > 0.0 { fps } else { 0.0 }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stash_direct_load_preserves_source_frame_rate() {
        let _guard = crate::testlock::serial();
        for fps in [23.976, 29.97, 60.0] {
            let scene: Scene = serde_json::from_value(serde_json::json!({
                "id": "synthetic",
                "paths": { "stream": "http://example.test/scene/synthetic/stream" },
                "files": [{ "width": 3840, "height": 2160,
                    "video_codec": "h264", "audio_codec": "aac", "frame_rate": fps }]
            })).unwrap();
            let stream = select_stream(&scene).unwrap();
            let mut session = route::PlaybackSession::default();
            assert!(route::prepare_stash_stream(&mut session, &scene.id, &stream.url,
                &stream.video, &stream.audio, stream.width, stream.height, stream.fps));
            assert_eq!(route::stream_fps(&session), fps);
        }
    }
    #[test]
    fn unknown_and_invalid_frame_rates_stay_unknown() {
        for fps in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(source_fps(fps), 0.0);
        }
        let file: crate::stash::SceneFile = serde_json::from_str(r#"{"frame_rate":null}"#).unwrap();
        assert_eq!(file.frame_rate, 0.0);
    }
    #[test]
    #[cfg(feature = "hostsim")]
    fn playback_adapter_respects_native_refusal_and_rejects_unstarted_seeks() {
        use std::sync::atomic::Ordering;
        let _guard = crate::testlock::serial();
        let old_paused = player::TX.paused.load(Ordering::Acquire);
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                player::force_pause_result_for_test(None);
                player::force_play_result_for_test(None);
                player::SHARED.reset_hls_clock_for_test();
                player::TX.commit_paused(self.0);
            }
        }
        let _restore = Restore(old_paused);
        player::SHARED.reset_hls_clock_for_test();
        player::TX.commit_paused(false);
        let mut playback = Playback::new(unsafe { crate::task::MainThread::assume() });
        player::force_pause_result_for_test(Some(0));
        playback.toggle_pause();
        assert!(!player::TX.paused.load(Ordering::Acquire));
        assert!(playback.sync_error.as_deref().unwrap().contains("pause"));
        player::force_pause_result_for_test(Some(1));
        playback.toggle_pause();
        assert!(player::TX.paused.load(Ordering::Acquire));
        player::force_play_result_for_test(Some(0));
        playback.toggle_pause();
        assert!(player::TX.paused.load(Ordering::Acquire));
        assert!(playback.sync_error.as_deref().unwrap().contains("resume"));
        player::force_play_result_for_test(Some(1));
        playback.toggle_pause();
        assert!(!player::TX.paused.load(Ordering::Acquire));
        playback.clock.position = 12.;
        playback.seek(42.);
        assert!(playback.sync_error.as_deref().unwrap().contains("seek"));
        assert_eq!(playback.clock.position, 12.);
        assert_eq!(playback.repause_at, None);
    }
    #[test]
    fn transport_toggle_uses_user_hold_and_reports_rejected_actuation() {
        let mut calls = Vec::new();
        // Derived Playing may be false while buffering/seeking; an unpaused intent
        // must still call Pause, not Resume. Only an accepted operation changes it.
        assert_eq!(
            toggle_transport(false, |pause| {
                calls.push(pause);
                true
            }),
            Ok(true)
        );
        assert_eq!(
            toggle_transport(true, |pause| {
                calls.push(pause);
                true
            }),
            Ok(false)
        );
        assert!(toggle_transport(false, |pause| {
            calls.push(pause);
            false
        })
        .is_err());
        assert_eq!(calls, [true, false, true]);
    }
    #[test]
    fn natural_completion_flushes_once_and_retains_counter_gate() {
        let mut history = HistoryTracker::new("scene-1".into());
        let _ = history.tick(Duration::from_secs(5), 98.0, true);
        let mut completed = false;
        assert_eq!(
            completion_flush(Some(&mut history), &mut completed),
            Some(HistoryCommand::SaveActivity {
                id: "scene-1".into(),
                resume_time: 0.0,
                watched_delta: 5.0,
            })
        );
        assert!(completed);
        assert!(completion_flush(Some(&mut history), &mut completed).is_none());
        assert_eq!(
            history.o_key(true),
            Some(HistoryCommand::AddO {
                id: "scene-1".into()
            })
        );
        assert!(history.o_key(true).is_none());
        history.o_key(false);
        assert!(history.o_key(true).is_none());
        history.o_key(false);
        history.o_finished();
        assert!(history.o_key(true).is_some());
    }
    #[test]
    fn incompatible_source_never_uses_direct_url() {
        let scene = Scene {
            paths: crate::stash::ScenePaths {
                stream: Some("http://example.test/direct".into()),
                ..Default::default()
            },
            files: vec![crate::stash::SceneFile {
                video_codec: "av1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(select_stream(&scene).is_none());
    }
    #[test]
    fn transcode_seek_replaces_offset_and_preserves_auth_query() {
        assert_eq!(offset_url("https://example.test/scene/synthetic/stream.mp4?apikey=synthetic-key&start=5&resolution=STANDARD_HD",12.25),"https://example.test/scene/synthetic/stream.mp4?apikey=synthetic-key&resolution=STANDARD_HD&start=12.250");
    }
    #[test]
    fn paused_marker_seek_waits_for_the_landed_frame() {
        let target = 60_000_000_000;
        assert!(!paused_seek_landed(target, target, 1, target));
        assert!(!paused_seek_landed(target, -1, 0, target));
        assert!(!paused_seek_landed(target, -1, 1, 0));
        assert!(paused_seek_landed(target, -1, 1, target));
    }
    #[test]
    fn native_clock_updates_are_not_divided_by_render_frame_rate() {
        let mut clock = ActivityClock::default();
        let mut watched = Duration::ZERO;
        for tick in 1..=60 {
            let position = (tick / 12) as f64 * 0.2;
            watched += clock.sample(Duration::from_secs_f64(1.0 / 60.0), position, true);
        }
        assert!((watched.as_secs_f64() - 1.0).abs() < 0.01);
    }
    #[test]
    fn clock_excludes_pause_buffering_and_seek_jumps() {
        let mut clock = ActivityClock::default();
        assert_eq!(
            clock.sample(Duration::from_secs(1), 0.0, true),
            Duration::ZERO
        );
        assert_eq!(
            clock.sample(Duration::from_secs(30), 0.0, false),
            Duration::ZERO
        );
        assert_eq!(
            clock.sample(Duration::from_millis(20), 300.0, true),
            Duration::ZERO
        );
        assert!(
            (clock
                .sample(Duration::from_millis(200), 300.2, true)
                .as_secs_f64()
                - 0.2)
                .abs()
                < 0.000001
        );
    }
    #[test]
    fn mp4_fallback_prefers_720p_and_never_uses_webm() {
        let scene = Scene {
            files: vec![crate::stash::SceneFile {
                video_codec: "av1".into(),
                audio_codec: "aac".into(),
                ..Default::default()
            }],
            scene_streams: vec![
                crate::stash::SceneStream {
                    url: "http://example.test/scene/synthetic/stream.mp4?resolution=FULL_HD".into(),
                    mime_type: Some("video/mp4".into()),
                    label: Some("MP4 Full HD (1080p)".into()),
                },
                crate::stash::SceneStream {
                    url: "http://example.test/scene/synthetic/stream.mp4?resolution=STANDARD_HD"
                        .into(),
                    mime_type: Some("video/mp4".into()),
                    label: Some("MP4 HD (720p)".into()),
                },
            ],
            ..Default::default()
        };
        let stream = select_stream(&scene).unwrap();
        assert!(stream.transcoded);
        assert!(stream.url.ends_with("STANDARD_HD"));
    }
}
