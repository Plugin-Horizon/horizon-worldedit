//! The noise of the expression functions `perlin`, `voronoi` and `ridgedmulti` (WorldEdit's `PerlinNoise`, `VoronoiNoise`
//! and `RidgedMultiFractalNoise`, which wrap libnoise), and `random`. The structure is libnoise's (gradient noise summed over
//! octaves, nearest-cell values, ridged weights) with its own table of gradient vectors: libnoise's table is a data file this
//! port does not have, so the values are coherent noise in the same range but not the same numbers.

use std::cell::Cell;

thread_local! {
    static RNG: Cell<u64> = Cell::new(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x1234_5678, |d| d.as_nanos() as u64) | 1,
    );
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `ThreadLocalRandom.nextDouble()`: a double in `[0, 1)`.
pub fn random() -> f64 {
    RNG.with(|r| {
        let mut s = r.get();
        let v = splitmix(&mut s);
        r.set(s);
        (v >> 11) as f64 / (1u64 << 53) as f64
    })
}

const X_NOISE_GEN: i32 = 1619;
const Y_NOISE_GEN: i32 = 31337;
const Z_NOISE_GEN: i32 = 6971;
const SEED_NOISE_GEN: i32 = 1013;

/// libnoise's `IntValueNoise3D`: a hash of the lattice point and the seed.
fn int_value_noise(x: i32, y: i32, z: i32, seed: i32) -> i32 {
    let mut n = X_NOISE_GEN.wrapping_mul(x).wrapping_add(Y_NOISE_GEN.wrapping_mul(y)).wrapping_add(Z_NOISE_GEN.wrapping_mul(z)).wrapping_add(SEED_NOISE_GEN.wrapping_mul(seed)) & 0x7fff_ffff;
    n = (n >> 13) ^ n;
    n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19_990_303)).wrapping_add(1_376_312_589) & 0x7fff_ffff
}

/// libnoise's `ValueNoise3D`: a value in `[-1, 1]` for a lattice point.
fn value_noise(x: i32, y: i32, z: i32, seed: i32) -> f64 {
    1.0 - f64::from(int_value_noise(x, y, z, seed)) / 1_073_741_824.0
}

/// The gradient vector of a lattice point (a table of 256 vectors made from a fixed generator).
fn gradient(index: i32) -> [f64; 3] {
    let mut s = (index as u64 & 0xff).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66_D1CE_4E5B;
    let mut v = [0.0; 3];
    loop {
        for c in &mut v {
            *c = (splitmix(&mut s) >> 11) as f64 / (1u64 << 52) as f64 - 1.0;
        }
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.1 && len <= 1.0 {
            return [v[0] / len, v[1] / len, v[2] / len];
        }
    }
}

/// libnoise's `GradientNoise3D`: the dot product of the lattice point's gradient and the offset to it.
fn gradient_noise(fx: f64, fy: f64, fz: f64, ix: i32, iy: i32, iz: i32, seed: i32) -> f64 {
    let mut index = X_NOISE_GEN.wrapping_mul(ix).wrapping_add(Y_NOISE_GEN.wrapping_mul(iy)).wrapping_add(Z_NOISE_GEN.wrapping_mul(iz)).wrapping_add(SEED_NOISE_GEN.wrapping_mul(seed)) & 0x7fff_ffffu32 as i32;
    index ^= index >> 8;
    let g = gradient(index & 0xff);
    (g[0] * (fx - f64::from(ix)) + g[1] * (fy - f64::from(iy)) + g[2] * (fz - f64::from(iz))) * 2.12
}

fn s_curve3(a: f64) -> f64 {
    a * a * (3.0 - 2.0 * a)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

/// libnoise's `GradientCoherentNoise3D` at standard quality.
fn coherent_noise(x: f64, y: f64, z: f64, seed: i32) -> f64 {
    let (x0, y0, z0) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let (x1, y1, z1) = (x0.wrapping_add(1), y0.wrapping_add(1), z0.wrapping_add(1));
    let (xs, ys, zs) = (s_curve3(x - f64::from(x0)), s_curve3(y - f64::from(y0)), s_curve3(z - f64::from(z0)));
    let n = |ix: i32, iy: i32, iz: i32| gradient_noise(x, y, z, ix, iy, iz, seed);
    let (n0, n1) = (n(x0, y0, z0), n(x1, y0, z0));
    let ix0 = lerp(n0, n1, xs);
    let (n0, n1) = (n(x0, y1, z0), n(x1, y1, z0));
    let ix1 = lerp(n0, n1, xs);
    let iy0 = lerp(ix0, ix1, ys);
    let (n0, n1) = (n(x0, y0, z1), n(x1, y0, z1));
    let ix0 = lerp(n0, n1, xs);
    let (n0, n1) = (n(x0, y1, z1), n(x1, y1, z1));
    let ix1 = lerp(n0, n1, xs);
    let iy1 = lerp(ix0, ix1, ys);
    lerp(iy0, iy1, zs)
}

/// libnoise's `MakeInt32Range`: keeps a coordinate in a range that cannot overflow.
fn int32_range(n: f64) -> f64 {
    if n >= 1_073_741_824.0 {
        2.0 * (n % 1_073_741_824.0) - 1_073_741_824.0
    } else if n <= -1_073_741_824.0 {
        2.0 * (n % 1_073_741_824.0) + 1_073_741_824.0
    } else {
        n
    }
}

const MAX_OCTAVES: i32 = 30;

fn octaves_ok(octaves: i32) -> Result<(), String> {
    if (1..=MAX_OCTAVES).contains(&octaves) {
        Ok(())
    } else {
        Err(format!("octave count must be between 1 and {MAX_OCTAVES}"))
    }
}

/// `perlin(seed, x, y, z, frequency, octaves, persistence)`, with libnoise's default lacunarity of 2.
pub fn perlin(seed: i32, at: [f64; 3], frequency: f64, octaves: i32, persistence: f64) -> Result<f64, String> {
    octaves_ok(octaves)?;
    let (mut x, mut y, mut z) = (at[0] * frequency, at[1] * frequency, at[2] * frequency);
    let (mut value, mut cur) = (0.0, 1.0);
    for octave in 0..octaves {
        let signal = coherent_noise(int32_range(x), int32_range(y), int32_range(z), seed.wrapping_add(octave) & 0x7fff_ffff);
        value += signal * cur;
        x *= 2.0;
        y *= 2.0;
        z *= 2.0;
        cur *= persistence;
    }
    Ok(value)
}

/// `voronoi(seed, x, y, z, frequency)`: the value of the cell nearest to the point (displacement 1, no distance term).
pub fn voronoi(seed: i32, at: [f64; 3], frequency: f64) -> Result<f64, String> {
    let (x, y, z) = (at[0] * frequency, at[1] * frequency, at[2] * frequency);
    let (xi, yi, zi) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let mut min = f64::from(i32::MAX);
    let mut candidate = [0.0; 3];
    for zc in zi - 2..=zi + 2 {
        for yc in yi - 2..=yi + 2 {
            for xc in xi - 2..=xi + 2 {
                let p = [f64::from(xc) + value_noise(xc, yc, zc, seed), f64::from(yc) + value_noise(xc, yc, zc, seed.wrapping_add(1)), f64::from(zc) + value_noise(xc, yc, zc, seed.wrapping_add(2))];
                let d = (p[0] - x).powi(2) + (p[1] - y).powi(2) + (p[2] - z).powi(2);
                if d < min {
                    min = d;
                    candidate = p;
                }
            }
        }
    }
    Ok(value_noise(candidate[0].floor() as i32, candidate[1].floor() as i32, candidate[2].floor() as i32, seed))
}

/// `ridgedmulti(seed, x, y, z, frequency, octaves)`, with libnoise's lacunarity of 2.
pub fn ridged_multi(seed: i32, at: [f64; 3], frequency: f64, octaves: i32) -> Result<f64, String> {
    octaves_ok(octaves)?;
    let (mut x, mut y, mut z) = (at[0] * frequency, at[1] * frequency, at[2] * frequency);
    let (mut value, mut weight) = (0.0, 1.0);
    let (offset, gain) = (1.0, 2.0);
    let mut spectral = 1.0f64;
    for octave in 0..octaves {
        let mut signal = coherent_noise(int32_range(x), int32_range(y), int32_range(z), seed.wrapping_add(octave) & 0x7fff_ffff);
        signal = offset - signal.abs();
        signal *= signal;
        signal *= weight;
        weight = (signal * gain).clamp(0.0, 1.0);
        value += signal * spectral.powf(-1.0);
        x *= 2.0;
        y *= 2.0;
        z *= 2.0;
        spectral *= 2.0;
    }
    Ok(value * 1.25 - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_stays_in_range() {
        let a = perlin(7, [1.3, 2.7, 3.1], 0.5, 4, 0.5).unwrap();
        assert_eq!(a, perlin(7, [1.3, 2.7, 3.1], 0.5, 4, 0.5).unwrap());
        assert_ne!(a, perlin(8, [1.3, 2.7, 3.1], 0.5, 4, 0.5).unwrap());
        for i in 0..200 {
            let p = [f64::from(i) * 0.37, f64::from(i) * 0.11, f64::from(i) * 0.73];
            assert!(perlin(3, p, 1.0, 1, 0.5).unwrap().abs() < 1.3);
            assert!(voronoi(3, p, 1.0).unwrap().abs() <= 1.0);
            let r = ridged_multi(3, p, 1.0, 3).unwrap();
            assert!((-1.5..=1.5).contains(&r), "{r}");
        }
        assert!(perlin(1, [0.0; 3], 1.0, 0, 0.5).is_err() && perlin(1, [0.0; 3], 1.0, 31, 0.5).is_err());
    }

    #[test]
    fn noise_is_smooth() {
        // Neighbouring points give close values.
        let a = perlin(5, [10.0, 10.0, 10.0], 1.0, 1, 0.5).unwrap();
        let b = perlin(5, [10.001, 10.0, 10.0], 1.0, 1, 0.5).unwrap();
        assert!((a - b).abs() < 0.05);
    }

    #[test]
    fn random_is_a_unit_double() {
        for _ in 0..100 {
            let r = random();
            assert!((0.0..1.0).contains(&r));
        }
    }
}
