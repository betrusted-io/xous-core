use nalgebra::{Matrix3, SVD};

pub type Matrix8x9<T> = nalgebra::Matrix<T, nalgebra::U8, nalgebra::U9, nalgebra::ArrayStorage<T, 8, 9>>;

// This function calculates the homography matrix from 4 corresponding point pairs
pub fn find_homography(src_points: [(f32, f32); 4], dst_points: [(f32, f32); 4]) -> Option<Matrix3<f32>> {
    // Prepare the A matrix (8x9) for 4 point pairs
    let mut a = Matrix8x9::zeros();
    let mut b = nalgebra::Vector::from_column_slice(&[0.0f32; 9]);

    for (i, (src, dst)) in src_points.iter().zip(dst_points.iter()).enumerate() {
        let (x, y) = *src;
        let (xp, yp) = *dst;
        let row1 = 2 * i;
        let row2 = 2 * i + 1;

        a[(row1, 0)] = -x;
        a[(row1, 1)] = -y;
        a[(row1, 2)] = -1.0;
        a[(row1, 6)] = x * xp;
        a[(row1, 7)] = y * xp;
        b[row1] = -xp;

        a[(row2, 3)] = -x;
        a[(row2, 4)] = -y;
        a[(row2, 5)] = -1.0;
        a[(row2, 6)] = x * yp;
        a[(row2, 7)] = y * yp;
        b[row2] = -yp;
    }

    // Solve the least-squares solution using Singular Value Decomposition (SVD)
    let svd = SVD::new_unordered(a.clone(), true, true);
    let solution = svd.solve(&b, f32::EPSILON);

    if let Ok(h) = solution {
        // Construct the homography matrix from the result
        let mut homography = Matrix3::identity();
        homography[(0, 0)] = h[0];
        homography[(0, 1)] = h[1];
        homography[(0, 2)] = h[2];
        homography[(1, 0)] = h[3];
        homography[(1, 1)] = h[4];
        homography[(1, 2)] = h[5];
        homography[(2, 0)] = h[6];
        homography[(2, 1)] = h[7];
        homography[(2, 2)] = 1.0;

        Some(homography)
    } else {
        None
    }
}

const HOM_FP_SHIFT: i32 = 18;
const HOM_FP_SCALE: i32 = 1 << HOM_FP_SHIFT;

pub fn matrix3_to_fixp(h: Matrix3<f32>) -> Matrix3<i32> {
    Matrix3::new(
        (h[(0, 0)] * HOM_FP_SCALE as f32) as i32,
        (h[(0, 1)] * HOM_FP_SCALE as f32) as i32,
        (h[(0, 2)] * HOM_FP_SCALE as f32) as i32,
        (h[(1, 0)] * HOM_FP_SCALE as f32) as i32,
        (h[(1, 1)] * HOM_FP_SCALE as f32) as i32,
        (h[(1, 2)] * HOM_FP_SCALE as f32) as i32,
        (h[(2, 0)] * HOM_FP_SCALE as f32) as i32,
        (h[(2, 1)] * HOM_FP_SCALE as f32) as i32,
        (h[(2, 2)] * HOM_FP_SCALE as f32) as i32,
    )
}

pub fn apply_fixp_homography(homography: &Matrix3<i32>, point: (i32, i32)) -> (i32, i32) {
    let (x, y) = point;

    // Transform the point using fixed-point arithmetic
    let tx = homography[(0, 0)] * x + homography[(0, 1)] * y + homography[(0, 2)];
    let ty = homography[(1, 0)] * x + homography[(1, 1)] * y + homography[(1, 2)];
    let tw = homography[(2, 0)] * x + homography[(2, 1)] * y + homography[(2, 2)];

    // Normalize by w to get the final coordinates
    if tw != 0 {
        let x_transformed = tx / tw;
        let y_transformed = ty / tw;

        (x_transformed, y_transformed)
    } else {
        log::info!(
            "Homography transformation would divide by 0: possible fixed-point underflow!\n\r  {}, {}\n\r  {} * {} + {} * {} + {}",
            tx,
            ty,
            homography[(2, 0)],
            x,
            homography[(2, 1)],
            y,
            homography[(2, 2)]
        );
        (0, 0)
    }
}
