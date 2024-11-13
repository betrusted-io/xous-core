use crate::Point;
use crate::qr::*;

/// This function takes a fraction described as `m.n`, where n is a fraction out of 1024, as well
/// as an `end` value, and returns numbers going from `0..end` such that with each
/// call of `next()` you produce a usize index in the range of `0..end` that is the
/// nearest algebraic integer that would result from adding the value `m.n` to the
/// current iteration count.
///
/// Thus, a value of 1.5 would be passed as m=1, n=512 (as 512/1024 = 0.5).
///
/// This "iterator" is not guaranteed to return every element in the range of `0..end`,
/// and in fact some numbers may not be returned at all, including `0` or `end-1`. However,
/// it does guarantee that the output is monatomically increasing, unless an error signal
/// is injected that is big enough to cause the count to go backwards.
///
/// Features that we might need:
///   - A report of the "error sign", expressed as +1, 0, -1, which expresses the direction
///    in which we're rounding. For example, 6.3 would be returned as 6 with an error of +1.
///    whereas 6.9 would be returned as 7 with an error of -1.
///   - The ability to inject an error correction signal into the fractional counter. This
///    if the loop internally concludes that an error signal would produce a more accurate
///    sub-pixel sampling, the error count would be applied at that iteration so that subsequent
///    indices absorb the delta into their future counts.

pub const FIXED_POINT_SHIFT: usize = 10;
pub const FIXED_POINT: usize = 1 << FIXED_POINT_SHIFT; // 1024

pub struct FracIter {
    stride: usize,
    current: usize,
    end: usize,
    count: usize,
    finished: bool,
}
impl FracIter {
    pub fn new(m: usize, n: usize, end: usize) -> Self {
        assert!(n < FIXED_POINT);
        // start the iterator at half the stride of a module
        Self {
            stride: (m << FIXED_POINT_SHIFT | n),
            current: (m << FIXED_POINT_SHIFT | n) / 2,
            end,
            count: 0,
            finished: false,
        }
    }

    pub fn next(&mut self) -> Option<usize> {
        // short circuit computation if we've hit the end of the iterator
        if self.finished {
            return None;
        }
        if self.count < self.end {
            let rounded_m = if (self.current & (FIXED_POINT - 1)) >= (FIXED_POINT / 2) {
                (self.current >> FIXED_POINT_SHIFT) + 1
            } else {
                self.current >> FIXED_POINT_SHIFT
            };
            self.current += self.stride;
            self.count += 1;
            Some(rounded_m)
        } else {
            self.finished = true;
            None
        }
    }

    pub fn reset(&mut self) {
        self.finished = false;
        self.current = self.stride / 2;
        self.count = 0;
    }

    pub fn error(&self) -> isize {
        if (self.current & (FIXED_POINT - 1)) == 0 {
            0
        } else if (self.current & (FIXED_POINT - 1)) >= (FIXED_POINT / 2) {
            1
        } else {
            -1
        }
    }

    /// The sign on m applies to n, but n is represented as a sign-less quantity
    pub fn nudge(&mut self, m: isize, n: usize) {
        assert!(n < FIXED_POINT);
        if m >= 0 {
            self.current += ((m as usize) << FIXED_POINT_SHIFT) | n;
        } else {
            // nudge down case
            self.current -= ((-m as usize) << FIXED_POINT_SHIFT) | n;
        }
    }
}

pub fn stream_to_grid(
    image: &ImageRoi,
    qr_size_pixels: usize,
    qr_size_modules: usize,
    margin: usize,
) -> Vec<bool> {
    let pix_per_module = ((qr_size_pixels - margin * 2) << FIXED_POINT_SHIFT) / qr_size_modules;
    let mut x_frac = FracIter::new(
        pix_per_module >> FIXED_POINT_SHIFT,
        pix_per_module & ((FIXED_POINT) - 1),
        qr_size_modules,
    );
    let mut y_frac = FracIter::new(
        pix_per_module >> FIXED_POINT_SHIFT,
        pix_per_module & ((FIXED_POINT) - 1),
        qr_size_modules,
    );
    let mut grid = Vec::<bool>::new();
    let mut i = 0;
    log::info!("{}", qr_size_modules);
    while let Some(y) = y_frac.next() {
        const FUDGE_X: usize = 0;
        const FUDGE_Y: usize = 0;
        while let Some(x) = x_frac.next() {
            if true {
                if image.data[(y + margin - FUDGE_Y) * image.width + (x + margin + FUDGE_X)]
                    < crate::BW_THRESH
                {
                    grid.push(true);
                } else {
                    grid.push(false);
                }
            } else {
                if image
                    .neighbor_luma(Point::new(x as isize + margin as isize, y as isize + margin as isize))
                    .unwrap()
                    < crate::BW_THRESH
                {
                    grid.push(true);
                } else {
                    grid.push(false);
                }
            }
        }
        x_frac.reset();
        print!("{} ", i);
        i += 1;
    }
    grid
}
