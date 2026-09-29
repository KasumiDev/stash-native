//! Bounded, cancellable software media workers; GL texture ownership stays on the UI thread.
use image::{AnimationDecoder, ImageDecoder};
use std::collections::{HashMap, HashSet};
#[cfg(not(test))]
use std::ffi::c_void;
use std::ffi::CString;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};
pub(crate) mod playback;
const BUDGET: usize = 64 * 1024 * 1024;
const ASSET_LIMIT: usize = 8 * 1024 * 1024;
const WORKERS: usize = 32;
const PREVIEW_LIMIT: i64 = 32 * 1024 * 1024;
fn reserve_counter(counter: &AtomicUsize, bytes: usize, limit: usize) -> bool {
    let mut used = counter.load(Ordering::Acquire);
    loop {
        let Some(next) = used.checked_add(bytes).filter(|&n| n <= limit) else {
            return false;
        };
        match counter.compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(actual) => used = actual,
        }
    }
}
#[derive(Debug)]
pub(crate) struct MediaFrame {
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    // Queued/polled buffers retain their reservation even after the producer exits.
    _allocation: Option<Arc<Vec<Allocation>>>,
}
struct Job {
    stop: Arc<AtomicBool>,
    rx: mpsc::Receiver<MediaFrame>,
    finished: Arc<AtomicBool>,
    retry: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
#[derive(Debug)]
struct Allocation {
    budget: Arc<AtomicUsize>,
    bytes: usize,
}
impl Allocation {
    fn reserve(budget: &Arc<AtomicUsize>, bytes: usize) -> Option<Self> {
        if !reserve_counter(budget, bytes, BUDGET) {
            return None;
        }
        Some(Self {
            budget: budget.clone(),
            bytes,
        })
    }
    fn shrink_to(&mut self, bytes: usize) {
        assert!(bytes <= self.bytes);
        self.budget.fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
    }
}
fn reserve_or_retry(
    budget: &Arc<AtomicUsize>,
    bytes: usize,
    blocked: &AtomicBool,
) -> Option<Allocation> {
    let allocation = Allocation::reserve(budget, bytes);
    if allocation.is_none() && bytes <= BUDGET {
        blocked.store(true, Ordering::Release);
    }
    allocation
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
/// Poll returns only live workers' frames; canceled results can never replace the next focus.
pub(crate) struct MediaManager {
    jobs: HashMap<String, Job>,
    images: HashMap<String, (String, bool)>,
    done: HashSet<String>,
    retry_at: HashMap<String, u64>,
    budget: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    focus: Option<(String, String, u64)>,
    preview: Option<Job>,
}
impl MediaManager {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            images: HashMap::new(),
            done: HashSet::new(),
            retry_at: HashMap::new(),
            budget: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            focus: None,
            preview: None,
        }
    }
    pub fn request_image(&mut self, key: &str, url: &str, animated: bool) {
        if self
            .images
            .get(key)
            .is_some_and(|v| v.0 != url || v.1 != animated)
        {
            self.jobs.remove(key);
            self.done.remove(key);
            self.retry_at.remove(key);
        }
        self.images.insert(key.into(), (url.into(), animated));
    }
    pub fn retain_images(&mut self, keys: &[String]) {
        let keep: HashSet<&str> = keys.iter().map(String::as_str).collect();
        self.images.retain(|k, _| keep.contains(k.as_str()));
        self.jobs.retain(|k, _| keep.contains(k.as_str()));
        self.done.retain(|k| keep.contains(k.as_str()));
        self.retry_at.retain(|k, _| keep.contains(k.as_str()));
    }
    pub fn focus_preview(&mut self, key: Option<&str>, url: Option<&str>, now_ms: u64) {
        let next = key.zip(url).filter(|(_, u)| !u.is_empty());
        if self
            .focus
            .as_ref()
            .map(|(k, u, _)| (k.as_str(), u.as_str()))
            == next
        {
            return;
        }
        self.stop_preview();
        self.focus = next.map(|(k, u)| (k.into(), u.into(), now_ms));
    }
    pub fn stop_preview(&mut self) {
        self.preview = None;
        self.focus = None;
    }
    pub fn poll(&mut self, now_ms: u64) -> Vec<MediaFrame> {
        if self.preview_ready(now_ms) {
            if let Some((key, url, _)) = &self.focus {
                if self.active.load(Ordering::Acquire) < WORKERS {
                    self.preview = spawn(
                        key.clone(),
                        url.clone(),
                        true,
                        true,
                        self.budget.clone(),
                        self.active.clone(),
                    );
                }
            }
        }
        for (key, (url, animated)) in &self.images {
            if self.active.load(Ordering::Acquire) >= WORKERS - 1 {
                break;
            }
            if !self.jobs.contains_key(key)
                && !self.done.contains(key)
                && self.retry_at.get(key).is_none_or(|&at| now_ms >= at)
            {
                if let Some(job) = spawn(
                    key.clone(),
                    url.clone(),
                    *animated,
                    false,
                    self.budget.clone(),
                    self.active.clone(),
                ) {
                    self.jobs.insert(key.clone(), job);
                }
            }
        }
        let mut frames = Vec::new();
        for job in self.jobs.values() {
            while let Ok(frame) = job.rx.try_recv() {
                frames.push(frame);
            }
        }
        if let Some(job) = &self.preview {
            while let Ok(frame) = job.rx.try_recv() {
                frames.push(frame);
            }
        }
        let finished: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.finished.load(Ordering::Acquire))
            .map(|(k, _)| k.clone())
            .collect();
        for key in finished {
            if self
                .jobs
                .remove(&key)
                .is_some_and(|job| job.retry.load(Ordering::Acquire))
            {
                self.retry_at.insert(key, now_ms.saturating_add(500));
            } else {
                self.done.insert(key);
            }
        }
        frames
    }
    fn preview_ready(&self, now_ms: u64) -> bool {
        self.preview.is_none()
            && self
                .focus
                .as_ref()
                .is_some_and(|(_, _, at)| now_ms.saturating_sub(*at) >= 700)
    }
}
fn spawn(
    key: String,
    url: String,
    animated: bool,
    preview: bool,
    budget: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
) -> Option<Job> {
    if !reserve_counter(&active, 1, WORKERS) {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let retry = Arc::new(AtomicBool::new(false));
    let r = retry.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let (s, f, a) = (stop.clone(), finished.clone(), active.clone());
    if std::thread::Builder::new()
        .name("stash media".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            if preview {
                decode_preview(&key, &url, &s, &tx, &budget);
            } else {
                decode_image(&key, &url, animated, &s, &tx, &budget, &r);
            }
            f.store(true, Ordering::Release);
            a.fetch_sub(1, Ordering::AcqRel);
        })
        .is_err()
    {
        active.fetch_sub(1, Ordering::AcqRel);
        return None;
    }
    Some(Job {
        stop,
        rx,
        finished,
        retry,
    })
}
fn deliver(tx: &mpsc::SyncSender<MediaFrame>, stop: &AtomicBool, frame: MediaFrame) -> bool {
    let mut frame = frame;
    loop {
        if stop.load(Ordering::Acquire) {
            return false;
        }
        match tx.try_send(frame) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Full(f)) => {
                frame = f;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return false,
        }
    }
}
fn wait(stop: &AtomicBool, delay: Duration) {
    let now = Instant::now();
    let end = now
        .checked_add(delay)
        .unwrap_or(now + Duration::from_secs(60));
    while !stop.load(Ordering::Acquire) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn webp_frames(
    bytes: &[u8],
    budget: &Arc<AtomicUsize>,
) -> Option<(Vec<image::Frame>, Vec<Allocation>)> {
    let mut decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    // image-webp deliberately ignores disposal clears unless a background is configured.
    // ANIM encodes BGRA; configure the canvas explicitly to honor disposal and alpha.
    let mut at: usize = 12;
    let mut background = [0, 0, 0, 0];
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let end = at.checked_add(8)?.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if &bytes[at..at + 4] == b"ANIM" && size >= 6 {
            let color = &bytes[at + 8..at + 12];
            background = [color[2], color[1], color[0], color[3]];
            break;
        }
        at = end.checked_add(size % 2)?;
    }
    if decoder.has_animation() {
        decoder.set_background_color(image::Rgba(background)).ok()?;
    }
    let (w, h) = decoder.dimensions();
    let size = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    if w > 4096 || h > 4096 || size > 16 * 1024 * 1024 {
        return None;
    }
    let composite = Allocation::reserve(budget, size.saturating_mul(3))?;
    let mut frames = Vec::new();
    let mut allocations = vec![composite];
    let mut truncated = false;
    for frame in decoder.into_frames() {
        let Some(allocation) = Allocation::reserve(budget, size) else {
            truncated = true;
            break;
        };
        let frame = frame.ok()?;
        allocations.push(allocation);
        frames.push(frame);
        if frames.len() >= 256 {
            truncated = true;
            break;
        }
    }
    if truncated {
        frames.truncate(1);
        allocations.truncate(2);
    }
    (!frames.is_empty()).then_some((frames, allocations))
}
#[cfg_attr(test, allow(dead_code))]
struct Source {
    url: String,
    hs: Box<crate::stream::HttpStream>,
    offset: i64,
    size: i64,
    stop: Arc<AtomicBool>,
    bytes: Option<Vec<u8>>,
    _allocation: Option<Allocation>,
    budget: Arc<AtomicUsize>,
}
impl Source {
    fn open(
        url: &str,
        offset: i64,
        stop: Arc<AtomicBool>,
        budget: &Arc<AtomicUsize>,
        blocked: &AtomicBool,
    ) -> Option<Self> {
        if url.starts_with("https://") {
            let mut allocation = reserve_or_retry(budget, ASSET_LIMIT, blocked)?;
            let response = crate::net::request(
                url,
                &[],
                "GET",
                None,
                crate::net::API,
                false,
                Some(ASSET_LIMIT),
                None,
            )?;
            if !response.ok()
                || response.body.is_empty()
                || offset >= response.body.len() as i64
                || stop.load(Ordering::Acquire)
            {
                return None;
            }
            if response.body.capacity() > ASSET_LIMIT {
                return None;
            }
            allocation.shrink_to(response.body.capacity());
            return Some(Self {
                url: url.into(),
                hs: crate::stream::http_stream_boxed(),
                offset,
                size: response.body.len() as i64,
                stop,
                bytes: Some(response.body),
                _allocation: Some(allocation),
                budget: budget.clone(),
            });
        }
        if !url.starts_with("http://") {
            return None;
        }
        let (origin, path) = crate::plex::origin::split(url);
        let host = CString::new(origin.host()).ok()?;
        let path = CString::new(if path.is_empty() { "/" } else { path }).ok()?;
        let extra = CString::new(format!("Range: bytes={offset}-\r\n")).ok()?;
        let mut hs = crate::stream::http_stream_boxed();
        if crate::stream::http_open(
            &mut *hs,
            host.as_ptr(),
            origin.port(),
            path.as_ptr(),
            extra.as_ptr(),
            "GET",
        ) < 0
        {
            return None;
        }
        let status = crate::stream::hs_status(&*hs);
        if !(status == 200 && offset == 0 || status == 206) {
            crate::stream::http_close(&mut *hs);
            return None;
        }
        let size = crate::stream::hs_content_length(&*hs).checked_add(offset)?;
        if size <= 0 || size > PREVIEW_LIMIT {
            crate::stream::http_close(&mut *hs);
            return None;
        }
        Some(Self {
            url: url.into(),
            hs,
            offset,
            size,
            stop,
            bytes: None,
            _allocation: None,
            budget: budget.clone(),
        })
    }
    fn read(&mut self, dst: &mut [u8]) -> i32 {
        if self.stop.load(Ordering::Acquire) {
            return -1;
        }
        if let Some(bytes) = &self.bytes {
            let at = self.offset as usize;
            let n = dst.len().min(bytes.len().saturating_sub(at));
            dst[..n].copy_from_slice(&bytes[at..at + n]);
            self.offset += n as i64;
            return n as i32;
        }
        let n = crate::stream::http_read(
            &mut *self.hs,
            dst.as_mut_ptr(),
            dst.len().min(i32::MAX as usize) as i32,
        );
        if n > 0 {
            self.offset += n as i64;
        }
        n
    }
    #[cfg(not(test))]
    fn seek(&mut self, at: i64) -> bool {
        if self.stop.load(Ordering::Acquire) || at < 0 || at > self.size {
            return false;
        }
        if self.bytes.is_some() {
            self.offset = at;
            return true;
        }
        let Some(next) = Self::open(
            &self.url,
            at,
            self.stop.clone(),
            &self.budget,
            &AtomicBool::new(false),
        ) else {
            return false;
        };
        *self = next;
        true
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        crate::stream::http_close(&mut *self.hs);
    }
}
fn decode_image(
    key: &str,
    url: &str,
    animated: bool,
    stop: &Arc<AtomicBool>,
    tx: &mpsc::SyncSender<MediaFrame>,
    budget: &Arc<AtomicUsize>,
    blocked: &AtomicBool,
) {
    let Some(mut source) = Source::open(url, 0, stop.clone(), budget, blocked) else {
        return;
    };
    if source.size as usize > ASSET_LIMIT {
        return;
    }
    let encoded_limit = source.size as usize;
    let Some(_encoded_allocation) = reserve_or_retry(budget, encoded_limit, blocked) else {
        return;
    };
    let mut bytes = Vec::with_capacity(encoded_limit);
    let mut chunk = [0u8; 8192];
    loop {
        let n = source.read(&mut chunk);
        if n < 0 {
            return;
        }
        if n == 0 {
            break;
        }
        if !append_asset_chunk(&mut bytes, &chunk[..n as usize], encoded_limit) {
            return;
        }
    }
    drop(source);
    if animated && image::guess_format(&bytes).ok() == Some(image::ImageFormat::WebP) {
        if let Some((frames, allocations)) = webp_frames(&bytes, budget) {
            let allocations = Arc::new(allocations);
            loop {
                for frame in &frames {
                    let delay = frame.delay().numer_denom_ms();
                    if !deliver(
                        tx,
                        stop,
                        MediaFrame {
                            key: key.into(),
                            width: frame.buffer().width(),
                            height: frame.buffer().height(),
                            rgba: frame.buffer().as_raw().clone(),
                            _allocation: Some(allocations.clone()),
                        },
                    ) {
                        return;
                    }
                    wait(
                        stop,
                        Duration::from_millis((delay.0 as u64 / delay.1.max(1) as u64).max(10)),
                    );
                }
                if frames.len() == 1 || stop.load(Ordering::Acquire) {
                    return;
                }
            }
        }
    }
    if let Some(frame) = static_frame(key, &bytes, budget, blocked) {
        let _ = deliver(tx, stop, frame);
    }
}
fn append_asset_chunk(bytes: &mut Vec<u8>, chunk: &[u8], reserved: usize) -> bool {
    if bytes
        .len()
        .checked_add(chunk.len())
        .is_none_or(|n| n > reserved)
    {
        return false;
    }
    bytes.extend_from_slice(chunk);
    true
}
fn static_frame(
    key: &str,
    bytes: &[u8],
    budget: &Arc<AtomicUsize>,
    blocked: &AtomicBool,
) -> Option<MediaFrame> {
    let Ok(reader) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()
    else {
        return None;
    };
    let mut reader = reader;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    let thumb_pixels = (width.min(1280) as usize).checked_mul(height.min(720) as usize)?;
    // Reserve decoded pixels, codec working storage and thumbnail/conversion/output buffers
    // before read_image can allocate. Small images pay their actual dimensions, not 16 MiB.
    let working = usize::try_from(decoder.total_bytes())
        .ok()?
        .checked_mul(2)?
        .checked_add(
            thumb_pixels.checked_mul(decoder.color_type().bytes_per_pixel() as usize + 8)?,
        )?;
    let mut allocation = reserve_or_retry(budget, working, blocked)?;
    let image = image::DynamicImage::from_decoder(decoder).ok()?;
    let thumb = image.thumbnail(width.min(1280), height.min(720));
    drop(image);
    let rgba = thumb.into_rgba8();
    let (output_width, output_height) = rgba.dimensions();
    let pixels = rgba.into_raw();
    allocation.shrink_to(pixels.capacity());
    Some(MediaFrame {
        key: key.into(),
        width: output_width,
        height: output_height,
        rgba: pixels,
        _allocation: Some(Arc::new(vec![allocation])),
    })
}
#[cfg(not(test))]
unsafe extern "C" {
    fn stash_preview_open(
        dir: *const libc::c_char,
        source: *mut c_void,
        read: unsafe extern "C" fn(*mut c_void, *mut u8, i32) -> i32,
        seek: unsafe extern "C" fn(*mut c_void, i64, i32) -> i64,
    ) -> *mut c_void;
    fn stash_preview_next(
        decoder: *mut c_void,
        rgba: *mut u8,
        w: *mut i32,
        h: *mut i32,
        pts: *mut i64,
    ) -> i32;
    fn stash_preview_close(decoder: *mut c_void);
}
#[cfg(not(test))]
unsafe extern "C" fn read_source(source: *mut c_void, out: *mut u8, len: i32) -> i32 {
    if len <= 0 {
        return -1;
    }
    let s = unsafe { &mut *source.cast::<Source>() };
    let n = s.read(unsafe { std::slice::from_raw_parts_mut(out, len as usize) });
    if n == 0 {
        -541478725
    } else {
        n
    }
}
#[cfg(not(test))]
unsafe extern "C" fn seek_source(source: *mut c_void, offset: i64, whence: i32) -> i64 {
    let s = unsafe { &mut *source.cast::<Source>() };
    if whence & 0x10000 != 0 {
        return s.size;
    }
    let at = match whence & !0x20000 {
        0 => offset,
        1 => match s.offset.checked_add(offset) {
            Some(at) => at,
            None => return -1,
        },
        2 => match s.size.checked_add(offset) {
            Some(at) => at,
            None => return -1,
        },
        _ => return -1,
    };
    if s.seek(at) {
        at
    } else {
        -1
    }
}
#[cfg(not(test))]
fn decode_preview(
    key: &str,
    url: &str,
    stop: &Arc<AtomicBool>,
    tx: &mpsc::SyncSender<MediaFrame>,
    budget: &Arc<AtomicUsize>,
) {
    let Some(mut source) = Source::open(url, 0, stop.clone(), budget, &AtomicBool::new(false))
    else {
        return;
    };
    let Some(_allocation) = Allocation::reserve(budget, 36 * 1024 * 1024) else {
        return;
    };
    let allocations = Arc::new(vec![_allocation]);
    let directory = crate::paths::app_dir().to_path_buf();
    #[cfg(feature = "hostsim")]
    let directory = if let Some(path) = std::env::var_os("PLX_FFMPEG_DIR") {
        std::path::PathBuf::from(path)
    } else if directory.join("ffmpeg-host").is_dir() {
        directory.join("ffmpeg-host")
    } else {
        directory
    };
    let Ok(dir) = CString::new(directory.to_string_lossy().as_bytes()) else {
        return;
    };
    let decoder = unsafe {
        stash_preview_open(
            dir.as_ptr(),
            (&mut source as *mut Source).cast(),
            read_source,
            seek_source,
        )
    };
    if decoder.is_null() {
        return;
    }
    let mut pixels = vec![0; 640 * 360 * 4];
    let mut w = 0;
    let mut h = 0;
    let mut pts = 0;
    let mut last_pts = -67;
    let mut first_pts = None;
    let mut clock = Instant::now();
    while !stop.load(Ordering::Acquire) {
        let n =
            unsafe { stash_preview_next(decoder, pixels.as_mut_ptr(), &mut w, &mut h, &mut pts) };
        if n < 0 {
            break;
        }
        if n == 0 {
            last_pts = -67;
            first_pts = None;
            clock = Instant::now();
            continue;
        }
        pts = pts.saturating_sub(*first_pts.get_or_insert(pts)).max(0);
        if pts.saturating_sub(last_pts) < 67 {
            continue;
        }
        last_pts = pts;
        if pts > 0 {
            wait(
                stop,
                Duration::from_millis(pts as u64).saturating_sub(clock.elapsed()),
            );
        }
        if w <= 0 || h <= 0 || w > 640 || h > 360 {
            break;
        }
        if !deliver(
            tx,
            stop,
            MediaFrame {
                key: key.into(),
                width: w as u32,
                height: h as u32,
                rgba: pixels[..w as usize * h as usize * 4].to_vec(),
                _allocation: Some(allocations.clone()),
            },
        ) {
            break;
        }
    }
    unsafe { stash_preview_close(decoder) };
}
#[cfg(test)]
fn decode_preview(
    _: &str,
    _: &str,
    _: &Arc<AtomicBool>,
    _: &mpsc::SyncSender<MediaFrame>,
    _: &Arc<AtomicUsize>,
) {
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_is_shared_and_reclaimed() {
        let budget = Arc::new(AtomicUsize::new(0));
        let a = Allocation::reserve(&budget, BUDGET).unwrap();
        assert!(Allocation::reserve(&budget, 1).is_none());
        drop(a);
        assert_eq!(budget.load(Ordering::Relaxed), 0);
    }
    fn tiny_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut bytes),
            &[12, 34, 56, 255].repeat(32 * 32),
            32,
            32,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        bytes
    }
    #[test]
    fn understated_content_length_cannot_grow_encoded_buffer() {
        let mut bytes = Vec::with_capacity(4);
        assert!(append_asset_chunk(&mut bytes, &[1, 2, 3], 4));
        assert!(!append_asset_chunk(&mut bytes, &[4, 5], 4));
        assert_eq!(bytes, [1, 2, 3]);
        assert_eq!(bytes.capacity(), 4);
        assert!(append_asset_chunk(&mut bytes, &[4], 4));
        assert!(!append_asset_chunk(&mut bytes, &[5], 4));
    }
    #[test]
    fn many_small_images_decode_concurrently_and_hold_output_budget() {
        let budget = Arc::new(AtomicUsize::new(0));
        let bytes = Arc::new(tiny_png());
        // HTTPS admission reserves its bounded request buffer, then releases the
        // unused capacity. Small responses must not permanently cost 8 MiB each.
        let mut encoded = Vec::new();
        for _ in 0..WORKERS {
            let mut allocation = Allocation::reserve(&budget, ASSET_LIMIT).unwrap();
            allocation.shrink_to(bytes.len());
            encoded.push(allocation);
        }
        let threads: Vec<_> = (0..WORKERS)
            .map(|id| {
                let (budget, bytes) = (budget.clone(), bytes.clone());
                std::thread::spawn(move || {
                    static_frame(&id.to_string(), &bytes, &budget, &AtomicBool::new(false)).unwrap()
                })
            })
            .collect();
        let frames: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(frames
            .iter()
            .all(|f| f.width == 32 && f.height == 32 && f.rgba[..4] == [12, 34, 56, 255]));
        let outputs: usize = frames.iter().map(|f| f.rgba.capacity()).sum();
        assert_eq!(
            budget.load(Ordering::Acquire),
            outputs + WORKERS * bytes.len()
        );
        drop(encoded);
        assert_eq!(budget.load(Ordering::Acquire), outputs);
        drop(frames);
        assert_eq!(budget.load(Ordering::Acquire), 0);
    }
    #[test]
    fn static_decode_retries_after_working_budget_is_released() {
        let budget = Arc::new(AtomicUsize::new(0));
        let occupied = Allocation::reserve(&budget, BUDGET).unwrap();
        let blocked = AtomicBool::new(false);
        let bytes = tiny_png();
        assert!(static_frame("a", &bytes, &budget, &blocked).is_none());
        assert!(blocked.load(Ordering::Acquire));
        assert_eq!(budget.load(Ordering::Acquire), BUDGET);
        drop(occupied);
        let frame = static_frame("a", &bytes, &budget, &AtomicBool::new(false)).unwrap();
        assert_eq!(budget.load(Ordering::Acquire), frame.rgba.capacity());
        drop(frame);
        assert_eq!(budget.load(Ordering::Acquire), 0);
    }
    #[test]
    fn budget_blocked_job_remains_pending_with_backoff() {
        let mut manager = MediaManager::new();
        manager.request_image("a", "http://example.test/a", false);
        // Suppress network workers; this test injects their completed result.
        manager.active.store(WORKERS, Ordering::Release);
        let (_, rx) = mpsc::sync_channel(1);
        manager.jobs.insert(
            "a".into(),
            Job {
                stop: Arc::new(AtomicBool::new(false)),
                rx,
                finished: Arc::new(AtomicBool::new(true)),
                retry: Arc::new(AtomicBool::new(true)),
            },
        );
        assert!(manager.poll(100).is_empty());
        assert!(!manager.done.contains("a"));
        assert_eq!(manager.retry_at["a"], 600);
        assert!(manager.poll(599).is_empty());
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(static_frame("a", &tiny_png(), &manager.budget, &AtomicBool::new(false)).unwrap())
            .unwrap();
        manager.jobs.insert(
            "a".into(),
            Job {
                stop: Arc::new(AtomicBool::new(false)),
                rx,
                finished: Arc::new(AtomicBool::new(true)),
                retry: Arc::new(AtomicBool::new(false)),
            },
        );
        let frames = manager.poll(600);
        assert_eq!(frames.len(), 1);
        assert!(manager.done.contains("a"));
    }
    #[test]
    fn focus_dwell_restarts_only_on_change() {
        let mut m = MediaManager::new();
        m.focus_preview(Some("a"), Some("http://example.test/a"), 100);
        m.focus_preview(Some("a"), Some("http://example.test/a"), 500);
        assert_eq!(m.focus.as_ref().unwrap().2, 100);
        m.focus_preview(Some("b"), Some("http://example.test/b"), 600);
        assert_eq!(m.focus.as_ref().unwrap().2, 600);
        m.stop_preview();
        assert!(m.focus.is_none());
    }
    #[test]
    fn preview_starts_after_exactly_seven_hundred_ms() {
        let mut m = MediaManager::new();
        m.focus_preview(Some("a"), Some("http://example.test/a"), 100);
        assert!(!m.preview_ready(799));
        assert!(m.preview_ready(800));
        m.stop_preview();
        assert!(!m.preview_ready(900));
    }
    #[test]
    fn invisible_images_are_canceled() {
        let mut m = MediaManager::new();
        m.request_image("a", "http://example.test/a", true);
        m.request_image("b", "http://example.test/b", false);
        m.retain_images(&["a".into()]);
        assert_eq!(m.images.len(), 1);
    }
    fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            bytes.push(0);
        }
        bytes
    }
    fn frame_chunk(width: u32, x: u32, color: [u8; 4], delay: u32, flags: u8) -> Vec<u8> {
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(
                &color.repeat(width as usize),
                width,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut header = Vec::new();
        for n in [x / 2, 0, width - 1, 0, delay] {
            header.extend_from_slice(&n.to_le_bytes()[..3]);
        }
        header.push(flags);
        // The lossless encoder writes one VP8L chunk after the RIFF/WEBP header.
        header.extend_from_slice(&webp[12..]);
        chunk(b"ANMF", &header)
    }
    fn animation_fixture() -> Vec<u8> {
        let mut payload = b"WEBP".to_vec();
        payload.extend(chunk(b"VP8X", &[0x12, 0, 0, 0, 2, 0, 0, 0, 0, 0]));
        payload.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
        payload.extend(frame_chunk(3, 0, [255, 0, 0, 255], 20, 2));
        payload.extend(frame_chunk(1, 0, [0, 0, 255, 255], 40, 3));
        payload.extend(frame_chunk(1, 2, [0, 255, 0, 128], 60, 0));
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend(payload);
        bytes
    }
    #[test]
    fn animated_webp_composites_disposal_blending_and_delays() {
        let budget = Arc::new(AtomicUsize::new(0));
        let bytes = animation_fixture();
        let (frames, allocations) = webp_frames(&bytes, &budget).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames
                .iter()
                .map(|f| f.delay().numer_denom_ms().0 / f.delay().numer_denom_ms().1)
                .collect::<Vec<_>>(),
            vec![20, 40, 60]
        );
        assert_eq!(frames[0].buffer().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(frames[1].buffer().get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(frames[2].buffer().get_pixel(0, 0).0, [0, 0, 0, 0]);
        let mixed = frames[2].buffer().get_pixel(2, 0).0;
        assert!((126..=128).contains(&mixed[0]) && (127..=129).contains(&mixed[1]));
        assert_eq!(mixed[3], 255);
        drop(frames);
        drop(allocations);
        assert_eq!(budget.load(Ordering::Acquire), 0);
    }
    #[test]
    fn animated_webp_budget_exhaustion_retains_first_still() {
        let budget = Arc::new(AtomicUsize::new(BUDGET - 48));
        let (frames, allocations) = webp_frames(&animation_fixture(), &budget).unwrap();
        assert_eq!(frames.len(), 1);
        drop(frames);
        drop(allocations);
        assert_eq!(budget.load(Ordering::Acquire), BUDGET - 48);
    }
    #[test]
    fn cancel_discards_queued_stale_preview() {
        let mut m = MediaManager::new();
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        tx.send(MediaFrame {
            key: "old".into(),
            width: 1,
            height: 1,
            rgba: vec![0; 4],
            _allocation: None,
        })
        .unwrap();
        m.preview = Some(Job {
            stop: stop.clone(),
            rx,
            finished: Arc::new(AtomicBool::new(false)),
            retry: Arc::new(AtomicBool::new(false)),
        });
        m.stop_preview();
        assert!(stop.load(Ordering::Acquire));
        assert!(m.poll(1000).is_empty());
    }
    #[test]
    fn bundled_decoder_reads_rgba_and_loops_synthetic_clip() {
        use std::ffi::c_void;
        unsafe extern "C" {
            fn stash_preview_open(
                directory: *const libc::c_char,
                source: *mut c_void,
                read: unsafe extern "C" fn(*mut c_void, *mut u8, i32) -> i32,
                seek: unsafe extern "C" fn(*mut c_void, i64, i32) -> i64,
            ) -> *mut c_void;
            fn stash_preview_next(
                decoder: *mut c_void,
                rgba: *mut u8,
                width: *mut i32,
                height: *mut i32,
                pts: *mut i64,
            ) -> i32;
            fn stash_preview_close(decoder: *mut c_void);
        }
        struct Memory {
            bytes: &'static [u8],
            at: usize,
        }
        unsafe extern "C" fn read(p: *mut c_void, out: *mut u8, n: i32) -> i32 {
            let s = unsafe { &mut *p.cast::<Memory>() };
            let n = (n.max(0) as usize).min(s.bytes.len() - s.at);
            if n == 0 {
                return -541478725;
            }
            unsafe { std::ptr::copy_nonoverlapping(s.bytes.as_ptr().add(s.at), out, n) };
            s.at += n;
            n as i32
        }
        unsafe extern "C" fn seek(p: *mut c_void, offset: i64, whence: i32) -> i64 {
            let s = unsafe { &mut *p.cast::<Memory>() };
            if whence & 0x10000 != 0 {
                return s.bytes.len() as i64;
            }
            let at = match whence & !0x20000 {
                0 => offset,
                1 => s.at as i64 + offset,
                2 => s.bytes.len() as i64 + offset,
                _ => return -1,
            };
            if at < 0 || at > s.bytes.len() as i64 {
                return -1;
            }
            s.at = at as usize;
            at
        }
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/ffmpeg-prefix-host/lib");
        if !directory.join("libavcodec-plx.so.63").exists()
            && !directory.join("libavcodec-plx.63.dylib").exists()
        {
            eprintln!(
                "native preview test requires bundled host FFmpeg; pure lifecycle tests still ran"
            );
            return;
        }
        let dir = CString::new(directory.to_string_lossy().as_bytes()).unwrap();
        let mut source = Memory {
            bytes: include_bytes!("fixtures/preview.mp4"),
            at: 0,
        };
        let decoder = unsafe {
            stash_preview_open(
                dir.as_ptr(),
                (&mut source as *mut Memory).cast(),
                read,
                seek,
            )
        };
        assert!(!decoder.is_null());
        let mut pixels = vec![0; 640 * 360 * 4];
        let (mut w, mut h, mut pts) = (0, 0, 0);
        let mut loops = 0;
        let mut frames = 0;
        for _ in 0..32 {
            let result = unsafe {
                stash_preview_next(decoder, pixels.as_mut_ptr(), &mut w, &mut h, &mut pts)
            };
            assert!(result >= 0);
            if result == 0 {
                loops += 1;
            } else {
                frames += 1;
                assert_eq!((w, h), (320, 180));
                assert!(pixels[..w as usize * h as usize * 4]
                    .iter()
                    .any(|&v| v != 0));
            }
        }
        unsafe { stash_preview_close(decoder) };
        assert!(loops >= 2 && frames >= 12);
    }
}
