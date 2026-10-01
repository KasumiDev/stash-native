//! Dedicated Stash loop and adapters over the shared dispatcher and platform.
use super::*;
use crate::screens::stash_registry::*;
use crate::stores::stash::Worker;
use crate::ui::dispatch::{CxParts, Dispatcher, NoTap, Rig, Split};
use crate::ui::frame::Budget;
use crate::ui::machine::Key;
use crate::ui::machine::*;
use crate::ui::present::{Present, Provenance};
use crate::ui::screen::ScreenArg;

use crate::stash::Config;
use crate::stash_media::MediaManager;
use std::collections::HashMap;
#[derive(Default)]
struct PointerIngress {
    button_down: bool,
    last_wheel: u32,
}
impl PointerIngress {
    fn motion(&mut self, kind: u32, x: f32, y: f32, tick: Tick) -> Vec<InputEvent<u32>> {
        vec![match kind {
            SDL_MOUSEBUTTONDOWN => {
                self.button_down = true;
                super::bridge::click_input(x, y, tick)
            }
            SDL_MOUSEBUTTONUP => {
                self.button_down = false;
                super::bridge::release_input(tick)
            }
            _ if self.button_down => super::bridge::drag_input(x, y, tick),
            _ => super::bridge::pointer_input(x, y, tick),
        }]
    }
    fn wheel(&mut self, dy: i32, tick: Tick) -> Vec<InputEvent<u32>> {
        if dy == 0 || tick.ms.wrapping_sub(self.last_wheel) <= 250 {
            return Vec::new();
        }
        self.last_wheel = tick.ms;
        super::bridge::wheel_input(dy, tick)
    }
}
fn register_work(
    dispatcher: &mut Dispatcher<StashHost>,
    work: &mut Vec<(Addr, crate::stores::stash::Work)>,
) -> Vec<(Addr, crate::stores::stash::Work)> {
    let pending = std::mem::take(work);
    for (addr, _) in &pending {
        if let MachineId::Instance(instance) = addr.to {
            dispatcher.track_inflight(instance, addr.req);
        }
    }
    pending
}
fn root_back(arg: Option<&StashArg>, configured: bool) -> Option<StashArg> {
    match arg {
        Some(StashArg::Home) => None,
        Some(StashArg::Settings) if !configured => Some(StashArg::Settings),
        _ => Some(StashArg::Home),
    }
}
/// Finite simulator scenes render the native HUD without starting media or writing history.
#[cfg(feature = "hostsim")]
fn fixture_playback(route: &str) -> Option<PlaybackView> {
    if !matches!(route, "player" | "ended" | "audio" | "subtitles") {
        return None;
    }
    Some(PlaybackView {
        scene: Some(crate::stash::Scene {
            id: "1".into(),
            title: Some("Scene 1 — a deliberately long title that must stay clear of subtitle, audio, and O count controls".into()),
            scene_markers: (0..8).map(|index| crate::stash::SceneMarker {
                id: format!("fixture-marker-{index}"),
                title: format!("Marker {}", index + 1),
                seconds: index as f64 * 120.,
                screenshot: Some(format!("fixture://marker/{index}")),
                ..Default::default()
            }).collect(),
            ..Default::default()
        }),
        position: 120.,
        duration: 1560.,
        playing: true,
        completed: route == "ended",
        o_count: 7,
        audio: vec![
            "English · AAC · Stereo".into(),
            "French · AAC · Stereo".into(),
        ],
        subtitles: vec!["English · SRT".into(), "French · SRT".into()],
        selected_audio: 0,
        selected_subtitle: -1,
        ..Default::default()
    })
}
struct StashRig {
    mount: Mount,
    worker: Worker,
    measure: crate::text::TtfMeasure,
    config: Config,
    textures: HashMap<String, (u32, f32, f32)>,
    media: MediaManager,
    pending: Vec<StashFx>,
    work: Vec<(Addr, crate::stores::stash::Work)>,
    playback: PlaybackView,
    chrome: super::stash_chrome::StashChrome,
    now: u32,
}
impl Rig<StashHost> for StashRig {
    fn split(&mut self) -> Split<'_, StashHost> {
        Split {
            mounter: &mut self.mount,
            views: Views {
                textures: &self.textures,
                config: &self.config,
                playback: &self.playback,
            },
            measure: &self.measure,
        }
    }
    fn deliver(
        &mut self,
        _: MachineId,
        _: &StashMsg,
        _: &CxParts<u32>,
        _: &mut Effects<'_, StashHost>,
    ) -> Handled {
        Handled::No
    }
    fn timer(
        &mut self,
        _: MachineId,
        _: TimerId,
        _: &CxParts<u32>,
        _: &mut Effects<'_, StashHost>,
    ) {
    }
    fn app_fx(
        &mut self,
        _: MachineId,
        fx: StashFx,
        _: &CxParts<u32>,
        _: &mut Effects<'_, StashHost>,
    ) {
        match fx {
            StashFx::Work(addr, work) => self.work.push((addr, work)),
            StashFx::Media(images, preview) => {
                self.media.set_cache_account(&self.config);
                let client = crate::stash::Client::new(self.config.clone()).ok();
                let mut keys = Vec::new();
                for (key, url) in images {
                    if cfg!(feature = "hostsim") && url.starts_with("fixture://") {
                        if !self.textures.contains_key(&key) {
                            let tex = fixture_texture(&key, 0, 0);
                            let (w, h) = fixture_dimensions(&key);
                            self.textures.insert(key.clone(), (tex, w as f32, h as f32));
                        }
                        keys.push(key);
                        continue;
                    }
                    if let Some(url) = client
                        .as_ref()
                        .and_then(|client| client.media_url(&url).ok())
                    {
                        self.media
                            .request_image(&key, &url, key.starts_with("performer:"));
                        keys.push(key);
                    }
                }
                if let Some((key, _)) = &preview {
                    keys.push(format!("preview:{key}"));
                }
                self.media.retain_images(&keys);
                let removed: Vec<_> = self
                    .textures
                    .keys()
                    .filter(|k| !keys.contains(k))
                    .cloned()
                    .collect();
                for key in removed {
                    if let Some((texture, _, _)) = self.textures.remove(&key) {
                        crate::gfx::delete_tex(texture);
                    }
                }
                if let Some((key, url)) = preview {
                    if let Some(url) = client
                        .as_ref()
                        .and_then(|client| client.media_url(&url).ok())
                    {
                        self.media.focus_preview(
                            Some(&format!("preview:{key}")),
                            Some(&url),
                            self.now as u64,
                        );
                    }
                } else {
                    self.media.stop_preview();
                }
            }
            StashFx::Keyboard(up) => {
                if up {
                    crate::textinput::start();
                } else {
                    crate::textinput::stop();
                }
            }
            other => self.pending.push(other),
        }
    }
    fn prepare(&mut self, _: &mut Budget, _: &mut Present) {}
    fn draw_chrome(
        &mut self,
        arg: &StashArg,
        _: &CxParts<u32>,
        nav: crate::ui::screen::NavPresentation,
        glass: Option<&mut crate::ui::frame::glass::GlassPlan>,
    ) {
        if arg.chrome() == Chrome::TabBar {
            if let Some(glass) = glass {
                self.chrome
                    .draw(crate::ui::Painter::root().alpha(nav.chrome_alpha), glass);
            }
        }
    }
    fn log(&mut self, line: &str) {
        crate::log(line)
    }
    fn ls2_pump(&mut self) {
        super::run::platform_pump();
    }
    fn opaque_route(&mut self, bound: bool) {
        super::run::rig_opaque_route(bound);
    }
    fn clear_opaque_region(&mut self) {
        super::run::rig_clear_opaque_region();
    }
    fn now_us(&self) -> u64 {
        self.now as u64 * 1000
    }
}
fn fixture_texture(key: &str, phase: u32, old: u32) -> u32 {
    let colors = [
        crate::ui::theme::TEXT_SECONDARY,
        crate::ui::theme::ACCENT,
        crate::ui::theme::CONTROL_IDLE_FILL,
    ];
    let seed = key.bytes().fold(0usize, |sum, b| sum + b as usize);
    let (w, h) = fixture_dimensions(key);
    let mut pixels = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let color = colors[((x / 24 + y / 24) + seed + phase as usize) % colors.len()];
            let p = (y * w + x) * 4;
            for c in 0..4 {
                pixels[p + c] = (color[c] * 255.) as u8;
            }
        }
    }
    crate::gfx::upload_rgba(old, w as i32, h as i32, pixels.as_ptr())
}
fn fixture_dimensions(key: &str) -> (usize, usize) {
    if key.contains("performer:") { (96, 144) }
    else if key.starts_with("scene:") && key.rsplit(':').next().and_then(|id| id.parse::<usize>().ok()).is_some_and(|id| id % 3 == 0) { (90, 160) }
    else if key.starts_with("tag:") { (128, 128) }
    else { (160, 90) }
}
fn scene_boot_route(route: &str) -> Option<StashArg> {
    route
        .strip_prefix("scene=")
        .filter(|id| !id.is_empty())
        .map(|id| StashArg::Scene(id.to_owned()))
}
/// Uses no Plex boot/session side effects. Runtime settings are loaded before frame ownership.
pub(crate) fn run() -> c_int {
    unsafe { run_inner() }
}
unsafe fn run_inner() -> c_int {
    let platform = match super::boot::platform(false) {
        Ok(p) => p,
        Err(e) => return e,
    };
    #[cfg(not(test))]
    crate::i18n::initialize(
        crate::i18n::Preference::System,
        cfg!(feature = "hostsim") && std::env::var("STASH_FIXTURES").as_deref() == Ok("1"),
    );
    let path = crate::paths::persistent_state_root().join("stash.json");
    let env = Config::from_env();
    let mut config = Config::load(&path)
        .or_else(|_| Config::load(&crate::paths::in_runtime_dir("stash.json")))
        .unwrap_or_default();
    if !env.server_url.is_empty() {
        config.server_url = env.server_url;
    }
    if let Ok(api_key) = std::env::var("STASH_API_KEY") {
        config.api_key = api_key;
    }
    config.apply_server_fallback(option_env!("STASH_DEFAULT_URL").unwrap_or_default());
    let mut rig = StashRig {
        mount: Mount,
        worker: Worker::new(config.clone(), path),
        measure: crate::text::TtfMeasure,
        config: config.clone(),
        textures: HashMap::new(),
        media: MediaManager::new(),
        pending: Vec::new(),
        work: Vec::new(),
        playback: PlaybackView::default(),
        chrome: super::stash_chrome::StashChrome::new(&crate::text::TtfMeasure),
        now: 0,
    };
    let mut dispatcher = Dispatcher::<StashHost>::new();
    let selected = std::env::var("STASH_SCREEN")
        .ok()
        .or_else(|| crate::dev::read("screen"))
        .map(|s| match s.trim() {
            route if scene_boot_route(route).is_some() => scene_boot_route(route).unwrap(),
            "performers" => StashArg::Performers,
            "scenes" => StashArg::Scenes,
            "galleries" => StashArg::Galleries,
            "tags" => StashArg::Tags,
            "search" => StashArg::Search,
            "settings" => StashArg::Settings,
            "scene" => StashArg::Scene("1".into()),
            "performer" => StashArg::Performer("1".into()),
            "tag" => StashArg::Tag("1".into()),
            "gallery" => StashArg::Gallery("1".into()),
            "viewer" => StashArg::Viewer {
                gallery: "1".into(),
                index: 0,
            },
            "player" | "ended" | "audio" | "subtitles" => StashArg::Player("1".into()),
            _ => StashArg::Home,
        })
        .unwrap_or(StashArg::Home);
    dispatcher.request(
        MachineId::Nav,
        NavOp::Root(
            if config.server_url.is_empty() && std::env::var("STASH_FIXTURES").as_deref() != Ok("1")
            {
                StashArg::Settings
            } else {
                selected
            },
        ),
    );
    let mt = crate::task::MainThread::assume();
    // Preserve the original native startup after platform/libcurl initialization.
    // FFmpeg refuses demux until its ABI check passes; webOS 4.x needs ACB to
    // attach decoded frames to the app's hardware video plane.
    crate::player::acb_init(&mt);
    crate::ff::boot();
    let mut playback = crate::stash_media::playback::Playback::new(mt);
    let mut remote = crate::remote::Remote::open();
    let mut previous = clock::now();
    let mut fixture_phase = 0;
    let mut running = true;
    let mut event = [0u8; 128];
    let mut pointer = PointerIngress::default();
    let mut glass = crate::ui::frame::glass::GlassPlan::new();
    let mut current_scene: Option<crate::stash::Scene> = None;
    let mut playback_error: Option<String> = None;
    let mut activity_generation = 0u32;
    #[cfg(feature = "hostsim")]
    let fixture_route = if std::env::var("STASH_FIXTURES").as_deref() == Ok("1") {
        std::env::var("STASH_SCREEN").unwrap_or_default()
    } else {
        String::new()
    };
    #[cfg(feature = "hostsim")]
    let mut fixture_panel_phase = 0u8;
    while running {
        #[cfg(all(feature = "hostsim", target_os = "linux"))]
        let pacing = platform
            .wslg_frame_pacing
            .then(crate::system::WslgFrameBudget::begin);
        if let Some(remote) = &mut remote {
            remote.drain(|token| {
                if token == "shot" {
                    #[cfg(feature = "hostsim")]
                    crate::shot::request();
                } else if let Some((sym, wcode)) = events::remote_token_key(token) {
                    events::remote_synth_key(sym, wcode);
                }
            });
        }
        let now = clock::now();
        let tick = Tick {
            ms: now,
            dt_us: now.wrapping_sub(previous).min(50) * 1000,
        };
        crate::ui::idle::frame_begin(tick.dt());
        previous = now;
        rig.now = now;
        if cfg!(feature = "hostsim")
            && std::env::var("STASH_FIXTURES").as_deref() == Ok("1")
            && now / 200 != fixture_phase
        {
            fixture_phase = now / 200;
            for (key, value) in &mut rig.textures {
                if key.starts_with("performer:") {
                    value.0 = fixture_texture(key, fixture_phase, value.0);
                    dispatcher
                        .present
                        .note(crate::ui::present::PresentEvent::Damage(Provenance::Input));
                }
            }
        }
        let mut inputs = Vec::new();
        while SDL_PollEvent(event.as_mut_ptr().cast()) != 0 {
            let kind = events::rd_u32(&event, 0);
            match kind {
                SDL_QUIT => running = false,
                SDL_KEYDOWN | SDL_KEYUP => {
                    let (state, wcode, sym) = events::decode_key(&event);
                    let edge = if kind == SDL_KEYUP {
                        Edge::Up
                    } else if state == 0x101 {
                        Edge::Repeat
                    } else {
                        Edge::Down
                    };
                    if wcode == 505 && edge == Edge::Down {
                        running = false;
                    }
                    if sym == 8 && edge != Edge::Up {
                        inputs.push(InputEvent {
                            at: tick,
                            source: Source::Sdl,
                            kind: InputKind::Text(TextEdit::Backspace),
                        });
                    } else {
                        let input = super::bridge::key_input(
                            sym,
                            wcode,
                            if kind == SDL_KEYUP { 0 } else { state },
                            tick,
                            Source::Sdl,
                        );
                        if matches!(
                            input.kind,
                            InputKind::Key {
                                key: Key::Up | Key::Down | Key::Left | Key::Right,
                                edge: Edge::Down,
                                ..
                            }
                        ) {
                            boot::hide_cursor();
                        }
                        inputs.push(input);
                    }
                }
                SDL_TEXTINPUT => {
                    let text = crate::textinput::decode(&event);
                    inputs.extend(events::text_inputs(&text, false, tick, Source::Sdl));
                }
                SDL_MOUSEMOTION | SDL_MOUSEBUTTONDOWN | SDL_MOUSEBUTTONUP => {
                    let x = events::rd_u32(&event, 20) as i32 as f32;
                    let y = events::rd_u32(&event, 24) as i32 as f32;
                    let (x, y) = crate::surface::to_logical(x, y);
                    inputs.extend(pointer.motion(kind, x, y, tick));
                }
                SDL_MOUSEWHEEL => {
                    let dy = if cfg!(feature = "hostsim") {
                        f32::from_bits(events::rd_u32(&event, 32)).round() as i32
                    } else {
                        events::rd_u32(&event, 20) as i32
                    };
                    if dy != 0 {
                        unsafe { boot::show_cursor() };
                        dispatcher.input.hit.note_pointer();
                    }
                    inputs.extend(pointer.wheel(dy, tick));
                }
                _ => {}
            }
        }
        for frame in rig.media.poll(now as u64) {
            let old = rig.textures.get(&frame.key).map_or(0, |t| t.0);
            let tex = crate::gfx::upload_rgba(
                old,
                frame.width as i32,
                frame.height as i32,
                frame.rgba.as_ptr(),
            );
            rig.textures
                .insert(frame.key, (tex, frame.width as f32, frame.height as f32));
            dispatcher
                .present
                .note(crate::ui::present::PresentEvent::Damage(Provenance::Input));
        }
        let previous_o_count = playback.o_count;
        playback.tick(now as u64);
        if playback.o_count != previous_o_count {
            activity_generation = activity_generation.wrapping_add(1);
            dispatcher.store_changed(STASH_ACTIVITY, activity_generation);
        }
        if let Some(scene) = current_scene.as_mut() {
            if playback.scene_id.as_deref() == Some(scene.id.as_str()) {
                scene.resume_time = if playback.completed() {
                    0.
                } else {
                    playback.position()
                };
                scene.o_counter = playback.o_count.unwrap_or(scene.o_counter);
            }
        }
        rig.playback = PlaybackView {
            scene: current_scene.clone(),
            position: playback.position(),
            duration: playback.duration(),
            playing: playback.playing(),
            loading: playback.loading(),
            completed: playback.completed(),
            o_count: playback.o_count.unwrap_or_default(),
            o_pending: playback.o_pending(),
            error: playback
                .error()
                .or_else(|| playback_error.clone())
                .or_else(|| playback.sync_error.clone())
                .unwrap_or_default(),
            audio: playback.audio_tracks(),
            subtitles: playback.subtitle_tracks(),
            selected_audio: playback.selected_audio(),
            selected_subtitle: playback.selected_subtitle(),
        };
        #[cfg(feature = "hostsim")]
        if let Some(view) = fixture_playback(&fixture_route) {
            rig.playback = view;
            if matches!(fixture_route.as_str(), "audio" | "subtitles")
                && matches!(dispatcher.top_arg(), Some(StashArg::Player(_)))
                && fixture_panel_phase < 2
            {
                let disc = crate::ui::player_hud::disc_hit_rect(if fixture_route == "audio" {
                    1
                } else {
                    0
                });
                let kind = if fixture_panel_phase == 0 {
                    SDL_MOUSEBUTTONDOWN
                } else {
                    SDL_MOUSEBUTTONUP
                };
                inputs.extend(pointer.motion(kind, disc.cx(), disc.cy(), tick));
                fixture_panel_phase += 1;
            }
        }
        rig.chrome.capture(&mut dispatcher, tick.dt());
        glass.step_tab_band(tick.dt());
        let results = rig.worker.poll();
        for (_, msg) in &results {
            if let StashMsg::Connected(Ok(config)) = msg {
                rig.media.set_cache_account(config);
                for (_, (texture, _, _)) in rig.textures.drain() {
                    crate::gfx::delete_tex(texture);
                }
                rig.config = config.clone();
            }
        }
        #[cfg(feature = "hostsim")]
        crate::shot::tick(now, false);
        dispatcher
            .present
            .note(crate::ui::present::PresentEvent::VideoPlane(
                playback.scene_id.is_some(),
            ));
        if crate::ui::idle::present_moving() {
            dispatcher
                .present
                .note(crate::ui::present::PresentEvent::Motion);
        }
        #[cfg(feature = "hostsim")]
        if std::env::var("STASH_FIXTURES").as_deref() == Ok("1") {
            // Finite capture fixtures need a deterministic presented-frame count, including
            // static catalog and paused/end-screen routes with no real video plane.
            dispatcher
                .present
                .note(crate::ui::present::PresentEvent::Damage(Provenance::Input));
        }
        crate::text::begin_frame();
        let report = dispatcher.frame_with(&mut rig, tick, inputs, results, &mut NoTap, false);
        if crate::ui::idle::present_moving() {
            // Shared card/chrome springs use the legacy motion reporter. Carry their last
            // step into the next dispatcher frame just as the original outer loop does.
            dispatcher
                .present
                .note(crate::ui::present::PresentEvent::Motion);
        }
        if report.back_at_root {
            match root_back(dispatcher.top_arg(), !rig.config.server_url.is_empty()) {
                Some(next) => {
                    if dispatcher.top_arg() != Some(&next) {
                        dispatcher.request(MachineId::Nav, NavOp::Root(next));
                    }
                }
                None => running = false,
            }
        }
        for (addr, work) in register_work(&mut dispatcher, &mut rig.work) {
            rig.worker.request(addr, work);
        }
        // Playback effects are consumed outside the dispatcher borrow and frame scope.
        for effect in std::mem::take(&mut rig.pending) {
            match effect {
                StashFx::Play(mut scene, resume) => {
                    if dispatcher.top_arg() != Some(&StashArg::Player(scene.id.clone())) {
                        continue;
                    }
                    if let Some(previous) = current_scene.as_ref().filter(|s| s.id == scene.id) {
                        scene.resume_time = previous.resume_time;
                        scene.o_counter = previous.o_counter;
                    }
                    current_scene = Some(scene.clone());
                    rig.media.stop_preview();
                    match crate::stash::Client::new(rig.config.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|client| playback.start(client, &scene, resume))
                    {
                        Ok(()) => {
                            playback_error = None;
                        }
                        Err(error) => {
                            playback_error = Some(error.clone());
                        }
                    }
                }
                StashFx::Player(action) => match action {
                    Action::Replay => {
                        if let Some(scene) = &current_scene {
                            if let Ok(client) = crate::stash::Client::new(rig.config.clone()) {
                                if let Err(e) = playback.start(client, scene, false) {
                                    playback_error = Some(e.clone());
                                } else {
                                    playback_error = None;
                                }
                            }
                        }
                    }
                    Action::SeekTo(position) => playback.seek(position),
                    Action::AudioTrack(ordinal) => playback.select_audio(ordinal),
                    Action::SubtitleTrack(ordinal) => playback.subtitle_track(ordinal),
                    Action::Pause => {
                        if playback.playing() {
                            playback.pause()
                        } else {
                            playback.resume()
                        }
                    }
                    Action::AddO => {
                        playback.o_key(true);
                        playback.o_key(false);
                    }
                    Action::Open(_) => playback.stop(),
                    _ => {}
                },
                _ => {}
            }
        }
        if report.presented {
            crate::surface::probe(platform.win);
            if matches!(dispatcher.top_arg(), Some(StashArg::Player(_))) {
                crate::gfx::frame_clear_through();
            } else {
                let (r, g, b) = crate::ui::theme::CLEAR_RGB;
                crate::gfx::frame_clear(r, g, b);
            }
            let _walk = glass.walk(crate::ui::frame::backdrop::Z::ALL);
            dispatcher.draw_with_glass_below(
                &mut rig,
                &mut glass,
                true,
                crate::ui::frame::backdrop::Z::ALL,
            );
            #[cfg(feature = "hostsim")]
            {
                let (vx, vy, vw, vh) = crate::surface::viewport();
                if crate::shot::maybe_capture(vx, vy, vw, vh) {
                    running = false;
                }
            }
            SDL_GL_SwapWindow(platform.win);
        }
        #[cfg(all(feature = "hostsim", target_os = "linux"))]
        if let Some(pacing) = pacing {
            pacing.finish();
        }
        if !report.presented {
            SDL_Delay(8);
        }
    }
    if let Err(error) = playback.shutdown() {
        crate::log(&error);
    }
    for (_, (texture, _, _)) in rig.textures.drain() {
        crate::gfx::delete_tex(texture);
    }
    SDL_Quit();
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Rect;
    #[test]
    fn explicit_scene_boot_preserves_the_requested_identity() {
        assert_eq!(
            scene_boot_route("scene=19"),
            Some(StashArg::Scene("19".into()))
        );
        assert_eq!(
            scene_boot_route("scene=4315"),
            Some(StashArg::Scene("4315".into()))
        );
        assert_eq!(scene_boot_route("scene="), None);
        assert_eq!(scene_boot_route("home"), None);
    }
    #[test]
    fn stash_boot_initializes_native_media_before_playback() {
        // Host seams cannot decode hardware video. Pin the real startup composition:
        // FFmpeg's ABI gate starts closed and ACB owns the webOS 4.x video plane.
        let source = include_str!("stash.rs");
        let startup = source.split("unsafe fn run_inner()").nth(1).unwrap();
        let startup = startup.split("let mut playback =").next().unwrap();
        let token = startup.find("let mt =").unwrap();
        let acb = startup
            .find("crate::player::acb_init(&mt)")
            .expect("ACB must initialize before Playback");
        let ffmpeg = startup
            .find("crate::ff::boot()")
            .expect("FFmpeg ABI gate must open before Playback");
        assert!(token < acb && acb < ffmpeg);
    }
    #[test]
    fn root_back_returns_home_and_preserves_connection_setup() {
        assert_eq!(
            root_back(Some(&StashArg::Scenes), true),
            Some(StashArg::Home)
        );
        assert_eq!(
            root_back(Some(&StashArg::Settings), false),
            Some(StashArg::Settings)
        );
        assert_eq!(
            root_back(Some(&StashArg::Settings), true),
            Some(StashArg::Home)
        );
        assert_eq!(root_back(Some(&StashArg::Home), true), None);
    }
    struct Measure;
    impl crate::ui::machine::Measure for Measure {
        fn width(&self, _: &std::ffi::CStr, _: i32, _: bool) -> f32 {
            10.
        }
        fn cap_h(&self, _: i32) -> f32 {
            10.
        }
        fn line_h(&self, _: i32) -> f32 {
            10.
        }
    }
    struct TestRig {
        mount: Mount,
        config: Config,
        textures: HashMap<String, (u32, f32, f32)>,
        work: Vec<(Addr, crate::stores::stash::Work)>,
        playback: PlaybackView,
    }
    impl Rig<StashHost> for TestRig {
        fn split(&mut self) -> Split<'_, StashHost> {
            Split {
                mounter: &mut self.mount,
                views: Views {
                    textures: &self.textures,
                    config: &self.config,
                    playback: &self.playback,
                },
                measure: &Measure,
            }
        }
        fn deliver(
            &mut self,
            _: MachineId,
            _: &StashMsg,
            _: &CxParts<u32>,
            _: &mut Effects<'_, StashHost>,
        ) -> Handled {
            Handled::No
        }
        fn timer(
            &mut self,
            _: MachineId,
            _: TimerId,
            _: &CxParts<u32>,
            _: &mut Effects<'_, StashHost>,
        ) {
        }
        fn app_fx(
            &mut self,
            _: MachineId,
            fx: StashFx,
            _: &CxParts<u32>,
            _: &mut Effects<'_, StashHost>,
        ) {
            if let StashFx::Work(addr, work) = fx {
                self.work.push((addr, work));
            }
        }
        fn log(&mut self, _: &str) {}
        fn prepare(&mut self, _: &mut Budget, _: &mut Present) {}
        fn ls2_pump(&mut self) {}
        fn opaque_route(&mut self, _: bool) {}
        fn clear_opaque_region(&mut self) {}
        fn now_us(&self) -> u64 {
            0
        }
    }
    #[test]
    fn registered_stash_query_reply_leaves_loading_and_publishes_scene() {
        let _lock = crate::testlock::serial();
        let mut rig = TestRig {
            mount: Mount,
            config: Config::default(),
            textures: HashMap::new(),
            work: Vec::new(),
            playback: PlaybackView::default(),
        };
        let mut dispatcher = Dispatcher::<StashHost>::new();
        dispatcher.request(MachineId::Nav, NavOp::Root(StashArg::Scenes));
        dispatcher.frame_with(
            &mut rig,
            Tick {
                ms: 0,
                dt_us: 16000,
            },
            Vec::new(),
            Vec::new(),
            &mut NoTap,
            false,
        );
        assert_eq!(rig.work.len(), 1);
        let initial_focus = dispatcher.focus().expect("navigation starts focused");
        let initial_screen = dispatcher
            .top_screen()
            .unwrap()
            .as_any()
            .unwrap()
            .downcast_ref::<crate::screens::stash::StashScreen>()
            .unwrap();
        assert_eq!(
            initial_screen.test_focus_identity(initial_focus.elem),
            Some("control:sort")
        );
        let pending = register_work(&mut dispatcher, &mut rig.work);
        let (addr, work) = pending.into_iter().next().unwrap();
        let generation = match work {
            crate::stores::stash::Work::Load { generation, .. } => generation,
            _ => panic!("expected initial query"),
        };
        assert!(dispatcher.nav.is_deliverable(&addr));
        let data = crate::stores::stash::PageData {
            title: "Scenes".into(),
            sections: vec![crate::stores::stash::Section {
                title: "Scenes".into(),
                portrait: false,
                shelf: false,
                tiles: (0..12)
                    .map(|i| crate::stores::stash::Tile {
                        identity: format!("scene:fixture{i}"),
                        title: format!("Fixture scene {i}"),
                        o_count: None,
                        caption: String::new(),
                        image: None,
                        preview: None,
                        action: Action::Open(StashArg::Scene("fixture".into())),
                    })
                    .collect(),
            }],
            count: 12,
            ..Default::default()
        };
        let report = dispatcher.frame_with(
            &mut rig,
            Tick {
                ms: 16,
                dt_us: 16000,
            },
            Vec::new(),
            vec![(
                addr,
                StashMsg::Loaded {
                    generation,
                    result: Ok(data),
                },
            )],
            &mut NoTap,
            false,
        );
        assert_eq!(report.dropped_deliveries, 0);
        let screen = dispatcher.top_screen().unwrap();
        let mut probe = String::new();
        screen.state().probe(&mut probe);
        assert!(probe.contains("loading=false"));
        let stash = screen
            .as_any()
            .unwrap()
            .downcast_ref::<crate::screens::stash::StashScreen>()
            .unwrap();
        assert!(stash.test_contains_content("scene:fixture0"));
        assert_eq!(dispatcher.focus(), Some(initial_focus));
        assert_eq!(
            stash.test_focus_identity(initial_focus.elem),
            Some("control:sort")
        );
        // Use the actual catalog's geometry and the same double-buffered map as a presented
        // frame. No injected semantic activation bypasses SDL's button-up ingress.
        let key = stash.test_content_key("scene:fixture0").unwrap();
        let rect = {
            let split = rig.split();
            let cx = Cx {
                views: split.views,
                measure: split.measure,
                tick: Tick::default(),
                press: PressRead::default(),
                focus: FocusRead::default(),
                owner: InputOwner::Entry(initial_focus.entry),
            };
            dispatcher
                .top_screen()
                .unwrap()
                .place(&key, &cx, crate::ui::screen::At::Drawn)
                .unwrap()
                .rect
        };
        dispatcher.input.hit.fill(vec![crate::ui::screen::Stop {
            key: FocusKey {
                entry: initial_focus.entry,
                elem: key,
            },
            rect,
            rest_rect: rect,
            clip: Rect::FULL,
            hover: crate::ui::screen::Hover::Focus,
            activate: crate::ui::screen::Activate::Press,
        }]);
        dispatcher.input.hit.swap();
        let mut ingress = PointerIngress::default();
        let tick = |ms| Tick { ms, dt_us: 16000 };
        for (ms, kind, x, y) in [
            (32, SDL_MOUSEMOTION, rect.cx(), rect.cy()),
            (48, SDL_MOUSEBUTTONDOWN, rect.cx(), rect.cy()),
            (64, SDL_MOUSEMOTION, 0., 0.),
            (80, SDL_MOUSEBUTTONUP, 0., 0.),
        ] {
            let at = tick(ms);
            dispatcher.frame_with(
                &mut rig,
                at,
                ingress.motion(kind, x, y, at),
                Vec::new(),
                &mut NoTap,
                false,
            );
        }
        for ms in (96..=400).step_by(16) {
            dispatcher.frame_with(
                &mut rig,
                tick(ms),
                Vec::new(),
                Vec::new(),
                &mut NoTap,
                false,
            );
        }
        assert_eq!(
            dispatcher.top_arg(),
            Some(&StashArg::Scenes),
            "moving away cancels a pointer press"
        );
        for (ms, kind) in [
            (416, SDL_MOUSEMOTION),
            (432, SDL_MOUSEBUTTONDOWN),
            (448, SDL_MOUSEBUTTONUP),
        ] {
            let at = tick(ms);
            dispatcher.frame_with(
                &mut rig,
                at,
                ingress.motion(kind, rect.cx(), rect.cy(), at),
                Vec::new(),
                &mut NoTap,
                false,
            );
        }
        for ms in (464..=1000).step_by(16) {
            dispatcher.frame_with(
                &mut rig,
                tick(ms),
                Vec::new(),
                Vec::new(),
                &mut NoTap,
                false,
            );
        }
        assert_eq!(
            dispatcher.top_arg(),
            Some(&StashArg::Scene("fixture".into())),
            "release opens the scene before the dropped-up timeout"
        );
        assert!(
            dispatcher.input.arm.is_none(),
            "one pointer click commits once"
        );
    }
    #[test]
    fn wheel_restores_hover_after_dpad_without_pointer_travel() {
        let _lock = crate::testlock::serial();
        let mut rig = TestRig {
            mount: Mount,
            config: Config::default(),
            textures: HashMap::new(),
            work: Vec::new(),
            playback: PlaybackView::default(),
        };
        let mut dispatcher = Dispatcher::<StashHost>::new();
        dispatcher.request(MachineId::Nav, NavOp::Root(StashArg::Scenes));
        let at = Tick {
            ms: 1000,
            dt_us: 16000,
        };
        dispatcher.frame_with(&mut rig, at, Vec::new(), Vec::new(), &mut NoTap, false);
        dispatcher.input.hit.note_dpad();
        dispatcher.frame_with(
            &mut rig,
            at,
            super::super::bridge::wheel_input(-1, at),
            Vec::new(),
            &mut NoTap,
            false,
        );
        assert!(
            !dispatcher.input.hit.dpad_mode,
            "wheel must activate hover immediately"
        );
        dispatcher.frame_with(
            &mut rig,
            at,
            vec![super::super::bridge::key_input(
                1073741905,
                81,
                1,
                at,
                Source::Sdl,
            )],
            Vec::new(),
            &mut NoTap,
            false,
        );
        assert!(
            dispatcher.input.hit.dpad_mode,
            "physical D-pad must suppress stray hover"
        );
    }
    #[test]
    fn pointer_drag_release_and_wheel_follow_original_bridge() {
        let mut ingress = PointerIngress::default();
        let tick = Tick { ms: 1000, dt_us: 0 };
        assert!(matches!(
            ingress.motion(SDL_MOUSEBUTTONDOWN, 1., 2., tick)[0].kind,
            InputKind::Click { .. }
        ));
        assert!(matches!(
            ingress.motion(SDL_MOUSEMOTION, 3., 4., tick)[0].kind,
            InputKind::Drag { .. }
        ));
        assert!(matches!(
            ingress.motion(SDL_MOUSEBUTTONUP, 3., 4., tick)[0].kind,
            InputKind::Key {
                key: Key::Ok,
                edge: Edge::Up,
                ..
            }
        ));
        assert!(matches!(
            ingress.motion(SDL_MOUSEMOTION, 3., 4., tick)[0].kind,
            InputKind::Pointer { .. }
        ));
        let wheel = ingress.wheel(-1, tick);
        assert!(matches!(
            wheel[0].kind,
            InputKind::Key {
                key: Key::Down,
                edge: Edge::Down,
                ..
            }
        ));
        assert!(matches!(
            wheel[1].kind,
            InputKind::Key {
                key: Key::Down,
                edge: Edge::Up,
                ..
            }
        ));
        assert!(ingress.wheel(1, Tick { ms: 1100, ..tick }).is_empty());
        assert!(matches!(
            ingress.wheel(1, Tick { ms: 1300, ..tick })[0].kind,
            InputKind::Key { key: Key::Up, .. }
        ));
    }
}
