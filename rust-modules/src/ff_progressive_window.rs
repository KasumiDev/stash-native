//! Bounded compressed-byte history for progressive AVIO. MP4 demux revisits
//! interleaved audio/video chunks; those byte seeks must not redial HTTP.
use std::collections::VecDeque;

pub(super) const CAPACITY: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ReadWindow {
    start: i64,
    bytes: VecDeque<u8>,
}

impl ReadWindow {
    pub(super) fn end(&self) -> i64 {
        self.start + self.bytes.len() as i64
    }

    pub(super) fn contains(&self, offset: i64) -> bool {
        offset >= self.start && offset <= self.end()
    }

    pub(super) fn reset(&mut self, offset: i64) {
        self.bytes.clear();
        self.start = offset;
    }

    pub(super) fn append(&mut self, offset: i64, data: &[u8]) {
        if offset != self.end() {
            self.reset(offset);
        }
        let data = if data.len() > CAPACITY {
            self.reset(offset + (data.len() - CAPACITY) as i64);
            &data[data.len() - CAPACITY..]
        } else {
            data
        };
        let evict = (self.bytes.len() + data.len()).saturating_sub(CAPACITY);
        self.bytes.drain(..evict);
        self.start += evict as i64;
        self.bytes.extend(data);
    }

    pub(super) fn read(&self, offset: i64, dst: &mut [u8]) -> usize {
        if !self.contains(offset) {
            return 0;
        }
        let index = (offset - self.start) as usize;
        let n = dst.len().min(self.bytes.len() - index);
        let (a, b) = self.bytes.as_slices();
        let first = n.min(a.len().saturating_sub(index));
        if first > 0 {
            dst[..first].copy_from_slice(&a[index..index + first]);
        }
        if first < n {
            let second = index.saturating_sub(a.len());
            dst[first..n].copy_from_slice(&b[second..second + n - first]);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewinds_survive_ring_wrap_and_storage_is_bounded() {
        let mut window = ReadWindow::default();
        let data: Vec<_> = (0..CAPACITY + 100).map(|i| (i % 251) as u8).collect();
        window.append(0, &data[..CAPACITY]);
        window.append(CAPACITY as i64, &data[CAPACITY..]);
        assert_eq!(window.bytes.len(), CAPACITY);
        assert!(window.bytes.capacity() <= CAPACITY);
        assert!(!window.contains(99));
        let mut dst = vec![0; 200];
        assert_eq!(window.read((CAPACITY - 100) as i64, &mut dst), 200);
        assert_eq!(dst, data[CAPACITY - 100..]);
        assert_eq!(window.read(window.end(), &mut dst), 0);
        window.reset(10000);
        assert_eq!(window.read(100, &mut dst), 0);
        window.append(10000, b"new stream bytes");
        assert_eq!(window.read(10000, &mut dst), 16);
        assert_eq!(&dst[..16], b"new stream bytes");
    }
}
