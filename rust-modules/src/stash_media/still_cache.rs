//! Disk-first still artwork. Animated sources persist only a static PNG snapshot.
use super::*;
use image::ImageEncoder;

pub(super) fn namespace(config: &crate::stash::Config) -> String {
    let Ok(endpoint) = config.endpoint() else {
        return String::new();
    };
    // API keys identify accounts on Stash. Neither the credential nor origin is stored in files.
    let identity = format!("{}\0{}", endpoint, config.api_key);
    crate::sha256::sha256(identity.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn snapshot(frame: &MediaFrame, budget: &Arc<AtomicUsize>) -> Option<(Vec<u8>, Allocation)> {
    let mut working = Allocation::reserve(budget, frame.rgba.len().checked_mul(3)?.max(4096))?;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &frame.rgba,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    if bytes.capacity() > working.bytes {
        return None;
    }
    working.shrink_to(bytes.capacity());
    Some((bytes, working))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_image(
    key: &str,
    url: &str,
    animated: bool,
    stop: &Arc<AtomicBool>,
    tx: &mpsc::SyncSender<MediaFrame>,
    budget: &Arc<AtomicUsize>,
    blocked: &AtomicBool,
    cache: Option<(u64, &crate::imgcache::DiskKey)>,
) {
    if let Some((generation, disk_key)) = cache {
        let Some(mut encoded) = reserve_or_retry(budget, 4 * 1024 * 1024, blocked) else { return };
        if let Some(hit) = crate::imgcache::read_at(generation, disk_key) {
            encoded.shrink_to(hit.bytes.capacity());
            if let Some(mut frame) = static_frame("cached-still", &hit.bytes, budget, blocked) {
                frame.key = key.into();
                if !deliver(tx, stop, frame) {
                    return;
                }
                // Animations always reload their transient stream after showing the cached still.
                if !animated && !hit.stale {
                    return;
                }
            } else {
                if blocked.load(Ordering::Acquire) { return; }
                crate::imgcache::remove_at(generation, disk_key);
            }
        }
    }
    if stop.load(Ordering::Acquire) {
        return;
    }
    let Some((bytes, _encoded)) = fetch_image(url, stop, budget, blocked) else {
        return;
    };
    // Persist validated, resized still pixels, never the animated compressed asset.
    if let Some(frame) = static_frame(key, &bytes, budget, blocked) {
        if let Some((generation, disk_key)) = cache {
            if !stop.load(Ordering::Acquire) {
                if let Some((png, _allocation)) = snapshot(&frame, budget) {
                    let _ = crate::imgcache::write_at(generation, disk_key, &png);
                }
            }
        }
        if !deliver(tx, stop, frame) {
            return;
        }
    }
    if animated
        && !stop.load(Ordering::Acquire)
        && image::guess_format(&bytes).ok() == Some(image::ImageFormat::WebP)
    {
        let _ = play_webp(key, &bytes, stop, tx, budget);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_portrait_snapshot_is_static_and_account_scoped() {
        let budget = Arc::new(AtomicUsize::new(0));
        let frame = MediaFrame {
            key: "portrait".into(),
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            _allocation: None,
        };
        let (bytes, encoded) = snapshot(&frame, &budget).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
        let decoded = static_frame("portrait", &bytes, &budget, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.rgba, frame.rgba);
        let a = crate::stash::Config {
            server_url: "http://example.test/graphql".into(),
            api_key: "a".into(),
        };
        let mut b = a.clone();
        b.api_key = "b".into();
        assert_ne!(namespace(&a), namespace(&b));
        assert_eq!(namespace(&a).len(), 64);
        drop(decoded);
        drop(encoded);
        assert_eq!(budget.load(Ordering::Acquire), 0);
    }
}
