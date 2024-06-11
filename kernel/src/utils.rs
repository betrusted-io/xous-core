/// Amount of stack-allocated space we allow for a MinSet pass. Increasing this
/// uses more memory, but reduces stack footprint. The current setting of 64
/// should use about 256 bytes of stack, which comfortably fits within the one-page
/// stack limit of the kernel.
pub const RENORM_PASS_SIZE: usize = 64;

/// A helper for assisting with renormalization of the epoch. This uses a small,
/// stack-allocated scratch space, limited by the size of `PASS_SIZE`, to find
/// a set of smallest values within a larger list. We can then use that set to
/// renormalize (e.g. compact down) those values to the smallest possible values,
/// while preserving duplicates and ordering within the overall list.
///
/// `dead_code` is allowed because we want this to be runnable in the set of
/// tests without having to enable the `swap` flag.
#[allow(dead_code)]
pub struct MinSet {
    /// Set is ordered from lowest to highest, once fully initialized
    pub set: [u32; RENORM_PASS_SIZE],
}
#[allow(dead_code)]
impl MinSet {
    pub fn new() -> Self { Self { set: [u32::MAX; RENORM_PASS_SIZE] } }

    pub fn insert(&mut self, item: u32) {
        // search from lowest to highest for a duplicate
        for s in self.set.iter_mut() {
            if *s == item {
                return;
            }
        }
        // it's not a duplicate
        // items are unique and sorted, find a spot to insert, if it deserves a place
        if item < self.set[RENORM_PASS_SIZE - 1] {
            self.set[RENORM_PASS_SIZE - 1] = item;
            self.set.sort_unstable();
        }
    }

    pub fn max(&self) -> u32 { self.set[RENORM_PASS_SIZE - 1] }

    pub fn index_of(&self, data: u32) -> Option<usize> {
        for (i, &d) in self.set.iter().enumerate() {
            if d == data {
                return Some(i);
            }
        }
        None
    }
}
