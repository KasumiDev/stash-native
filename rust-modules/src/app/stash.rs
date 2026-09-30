//! Dedicated Stash loop and adapters over the shared dispatcher and platform.
use super::*;
use crate::screens::stash_registry::*;
use crate::stores::stash::Worker;
use crate::ui::dispatch::{CxParts, Dispatcher, NoTap, Rig, Split};
use crate::ui::frame::Budget;
use crate::ui::machine::Key;
use crate::ui::machine::*;
use crate::ui::present::{Present, Provenance};

use crate::stash::Config;
use crate::stash_media::MediaManager;
use std::collections::HashMap;
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
struct StashRig {
    mount: Mount,
    worker: Worker,
    measure: crate::text::TtfMeasure,
    config: Config,
    textures: HashMap<String, (u32, f32, f32)>,
    media: MediaManager,
    pending: Vec<StashFx>,
    work: Vec<(Addr, crate::stores::stash::Work)>,
    status: String,
    now: u32,
}
impl Rig<StashHost> for StashRig {
    fn split(&mut self) -> Split<'_, StashHost> {
        Split {
            mounter: &mut self.mount,
            views: Views {
                textures: &self.textures,
                config: &self.config,
                player_status: &self.status,
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
                let client = crate::stash::Client::new(self.config.clone()).ok();
                let mut keys = Vec::new();
                for (key, url) in images {
                    if cfg!(feature = "hostsim") && url.starts_with("fixture://") {
                        if !self.textures.contains_key(&key) {
                            let tex = fixture_texture(&key, 0, 0);
                            self.textures.insert(key.clone(), (tex, 96., 144.));
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
    let mut pixels = vec![0u8; 96 * 144 * 4];
    for y in 0..144 {
        for x in 0..96 {
            let color = colors[((x / 24 + y / 24) + seed + phase as usize) % colors.len()];
            let p = (y * 96 + x) * 4;
            for c in 0..4 {
                pixels[p + c] = (color[c] * 255.) as u8;
            }
        }
    }
    crate::gfx::upload_rgba(old, 96, 144, pixels.as_ptr())
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
        status: String::new(),
        now: 0,
    };
    let mut dispatcher = Dispatcher::<StashHost>::new();
    let selected = std::env::var("STASH_SCREEN")
        .ok()
        .or_else(|| crate::dev::read("screen"))
        .map(|s| match s.trim() {
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
    let mut playback = crate::stash_media::playback::Playback::new(mt);
    let mut remote = crate::remote::Remote::open();
    let mut previous = clock::now();
    let mut fixture_phase = 0;
    let mut running = true;
    let mut event = [0u8; 128];
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
                    let key = match sym {
                        1073741906 => Key::Up,
                        1073741905 => Key::Down,
                        1073741904 => Key::Left,
                        1073741903 => Key::Right,
                        13 => Key::Ok,
                        27 | 8 => Key::Back,
                        _ => match wcode {
                            82 => Key::Up,
                            81 => Key::Down,
                            80 => Key::Left,
                            79 => Key::Right,
                            40 => Key::Ok,
                            482 => Key::Back,
                            _ => Key::Other,
                        },
                    };
                    inputs.push(InputEvent {
                        at: tick,
                        source: Source::Sdl,
                        kind: if sym == 8 && edge != Edge::Up {
                            InputKind::Text(TextEdit::Backspace)
                        } else {
                            InputKind::Key {
                                key,
                                sym,
                                wcode,
                                edge,
                                at_edge: false,
                            }
                        },
                    });
                }
                SDL_TEXTINPUT => {
                    let text = crate::textinput::decode(&event);
                    inputs.extend(events::text_inputs(&text, false, tick, Source::Sdl));
                }
                SDL_MOUSEMOTION | SDL_MOUSEBUTTONDOWN => {
                    let x = events::rd_u32(&event, 20) as i32 as f32;
                    let y = events::rd_u32(&event, 24) as i32 as f32;
                    let (x, y) = crate::surface::to_logical(x, y);
                    inputs.push(InputEvent {
                        at: tick,
                        source: Source::Sdl,
                        kind: if kind == SDL_MOUSEMOTION {
                            InputKind::Pointer { x, y, hit: None }
                        } else {
                            InputKind::Click { x, y, hit: None }
                        },
                    });
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
        playback.tick(now as u64);
        let results = rig.worker.poll();
        for (_, msg) in &results {
            if let StashMsg::Connected(Ok(config)) = msg {
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
        crate::ui::idle::frame_begin(tick.dt());
        crate::text::begin_frame();
        let report = dispatcher.frame_with(&mut rig, tick, inputs, results, &mut NoTap, false);
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
                StashFx::Play(scene, resume) => {
                    rig.media.stop_preview();
                    match crate::stash::Client::new(rig.config.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|client| playback.start(client, &scene, resume))
                    {
                        Ok(()) => rig.status = scene.display_title().to_owned(),
                        Err(error) => rig.status = error,
                    }
                }
                StashFx::Player(action) => match action {
                    Action::Pause => {
                        if playback.playing() {
                            playback.pause()
                        } else {
                            playback.resume()
                        }
                    }
                    Action::Seek(delta) => {
                        playback.seek((playback.position() + delta as f64).max(0.))
                    }
                    Action::AddO => {
                        playback.o_key(true);
                        playback.o_key(false);
                    }
                    Action::Open(_) => playback.stop(),
                    Action::Audio => playback.cycle_audio(),
                    Action::Subtitle => playback.cycle_subtitle(),
                    _ => {}
                },
                _ => {}
            }
        }
        if playback.scene_id.is_some() {
            rig.status = format!(
                "{} · {:.0}:{:02.0} / {:.0}:{:02.0} · O {}",
                if playback.playing() {
                    "Playing"
                } else {
                    "Paused"
                },
                (playback.position() / 60.).floor(),
                playback.position() % 60.,
                (playback.duration() / 60.).floor(),
                playback.duration() % 60.,
                playback.o_count.unwrap_or_default()
            );
        }
        if let Some(error) = playback.error().or_else(|| playback.sync_error.clone()) {
            rig.status = error;
        }
        if report.presented {
            crate::surface::probe(platform.win);
            if matches!(dispatcher.top_arg(), Some(StashArg::Player(_))) {
                crate::gfx::frame_clear_through();
            } else {
                let (r, g, b) = crate::ui::theme::CLEAR_RGB;
                crate::gfx::frame_clear(r, g, b);
            }
            dispatcher.draw(&mut rig, true);
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
    }
    impl Rig<StashHost> for TestRig {
        fn split(&mut self) -> Split<'_, StashHost> {
            Split {
                mounter: &mut self.mount,
                views: Views {
                    textures: &self.textures,
                    config: &self.config,
                    player_status: "",
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
            Some("control:nav:Home")
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
                tiles: (0..12)
                    .map(|i| crate::stores::stash::Tile {
                        identity: format!("scene:fixture{i}"),
                        title: format!("Fixture scene {i}"),
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
            Some("control:nav:Home")
        );
    }
}
