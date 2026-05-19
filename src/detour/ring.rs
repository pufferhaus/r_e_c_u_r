//! Contiguous frame ring. Pre-allocated slab + circular write index.
//!
//! `push` saturates `count` at `capacity`; `get(0)` always returns the oldest
//! retained frame, `get(count - 1)` the newest.

pub struct Ring {
    slab: Box<[u8]>,
    capacity: usize,
    write_index: usize,
    count: usize,
    bytes_per_frame: usize,
    width: u32,
    height: u32,
}

impl Ring {
    pub fn new(width: u32, height: u32, budget_bytes: usize) -> Self {
        let bytes_per_frame = (width as usize) * (height as usize) * 4;
        let capacity = if bytes_per_frame == 0 {
            0
        } else {
            budget_bytes / bytes_per_frame
        };
        let slab = vec![0u8; capacity * bytes_per_frame].into_boxed_slice();
        Self {
            slab,
            capacity,
            write_index: 0,
            count: 0,
            bytes_per_frame,
            width,
            height,
        }
    }

    pub fn push(&mut self, rgba: &[u8]) {
        let _ = self.try_push(rgba);
    }

    pub fn try_push(&mut self, rgba: &[u8]) -> bool {
        if self.capacity == 0 || rgba.len() != self.bytes_per_frame {
            return false;
        }
        let offset = self.write_index * self.bytes_per_frame;
        self.slab[offset..offset + self.bytes_per_frame].copy_from_slice(rgba);
        self.write_index = (self.write_index + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
        true
    }

    pub fn get(&self, i: usize) -> Option<&[u8]> {
        if i >= self.count {
            return None;
        }
        let slot = if self.count < self.capacity {
            i
        } else {
            (self.write_index + i) % self.capacity
        };
        let offset = slot * self.bytes_per_frame;
        Some(&self.slab[offset..offset + self.bytes_per_frame])
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_frame
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn latest_ring_index(&self) -> usize {
        self.count.saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_allocates_capacity_for_budget() {
        let r = Ring::new(720, 480, 4 * 1024 * 1024);
        assert_eq!(r.capacity(), 3);
        assert_eq!(r.count(), 0);
        assert_eq!(r.bytes_per_frame(), 720 * 480 * 4);
    }

    #[test]
    fn new_with_zero_budget_has_capacity_zero() {
        let r = Ring::new(8, 8, 0);
        assert_eq!(r.capacity(), 0);
    }

    #[test]
    fn push_increments_count_until_saturated() {
        let mut r = Ring::new(2, 2, 2 * 2 * 4 * 3);
        let frame = vec![1u8; 16];
        r.push(&frame);
        assert_eq!(r.count(), 1);
        r.push(&frame);
        r.push(&frame);
        assert_eq!(r.count(), 3);
        r.push(&frame);
        assert_eq!(r.count(), 3);
    }

    #[test]
    fn get_returns_oldest_first() {
        let mut r = Ring::new(1, 1, 4 * 3);
        r.push(&[1, 1, 1, 1]);
        r.push(&[2, 2, 2, 2]);
        r.push(&[3, 3, 3, 3]);
        assert_eq!(r.get(0).unwrap(), &[1, 1, 1, 1]);
        assert_eq!(r.get(1).unwrap(), &[2, 2, 2, 2]);
        assert_eq!(r.get(2).unwrap(), &[3, 3, 3, 3]);
        assert!(r.get(3).is_none());
    }

    #[test]
    fn wrap_drops_oldest() {
        let mut r = Ring::new(1, 1, 4 * 3);
        r.push(&[1, 1, 1, 1]);
        r.push(&[2, 2, 2, 2]);
        r.push(&[3, 3, 3, 3]);
        r.push(&[4, 4, 4, 4]);
        assert_eq!(r.get(0).unwrap(), &[2, 2, 2, 2]);
        assert_eq!(r.get(2).unwrap(), &[4, 4, 4, 4]);
    }

    #[test]
    fn latest_ring_index_is_count_minus_one() {
        let mut r = Ring::new(1, 1, 4 * 2);
        assert_eq!(r.latest_ring_index(), 0);
        r.push(&[1, 1, 1, 1]);
        assert_eq!(r.latest_ring_index(), 0);
        r.push(&[2, 2, 2, 2]);
        assert_eq!(r.latest_ring_index(), 1);
    }

    #[test]
    fn push_wrong_size_does_not_panic_in_release_but_returns_err_in_debug() {
        let mut r = Ring::new(1, 1, 4 * 2);
        let ok = r.try_push(&[0u8; 3]);
        assert!(!ok);
        assert_eq!(r.count(), 0);
    }
}
