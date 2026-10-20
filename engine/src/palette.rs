//! Limiting shape colors to a palette: a few colors chosen from the image
//! (by median cut, which also picks the colors of GIFs, and k-means), or
//! colors given.

use crate::core::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    colors: Vec<Color>,
}

impl Palette {
    /// A palette of the given colors, opaque; None if there are none.
    pub fn new(colors: Vec<Color>) -> Option<Palette> {
        if colors.is_empty() {
            return None;
        }
        let colors = colors.into_iter().map(|c| Color { a: 255, ..c }).collect();
        Some(Palette { colors })
    }

    /// At most n colors (fewer if the image has fewer) that represent the
    /// RGBA pixels: median cut's, refined by rounds of k-means, which fit
    /// the image more closely (shapes came out 2-9% closer to it).
    pub fn from_image(pix: &[u8], n: usize) -> Palette {
        const ROUNDS: usize = 10;
        let colors = median_cut(pix, n.max(1))
            .into_iter()
            .map(|[r, g, b]| Color {
                r: r as i32,
                g: g as i32,
                b: b as i32,
                a: 255,
            })
            .collect();
        let mut palette = Palette { colors };
        for _ in 0..ROUNDS {
            // move each color to the mean of the pixels nearest to it
            let mut sums = vec![[0i64; 4]; palette.colors.len()];
            for p in pix.as_chunks::<4>().0 {
                let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
                let s = &mut sums[palette.nearest_index(Color { r, g, b, a: 255 })];
                for (sum, v) in s.iter_mut().zip([r, g, b, 1]) {
                    *sum += v as i64;
                }
            }
            for (c, s) in palette.colors.iter_mut().zip(&sums) {
                if s[3] > 0 {
                    (c.r, c.g, c.b) = (
                        (s[0] / s[3]) as i32,
                        (s[1] / s[3]) as i32,
                        (s[2] / s[3]) as i32,
                    );
                }
            }
        }
        palette
    }

    pub fn colors(&self) -> &[Color] {
        &self.colors
    }

    /// The palette color nearest to c, whose channels may lie outside 0 to
    /// 255.
    pub fn nearest(&self, c: Color) -> Color {
        self.colors[self.nearest_index(c)]
    }

    fn nearest_index(&self, c: Color) -> usize {
        let dist = |p: &Color| {
            let (dr, dg, db) = ((p.r - c.r) as i64, (p.g - c.g) as i64, (p.b - c.b) as i64);
            dr * dr + dg * dg + db * db
        };
        (0..self.colors.len())
            .min_by_key(|&i| dist(&self.colors[i]))
            .unwrap()
    }
}

/// Builds a palette of at most n colors for RGBA pixels using median cut
/// over a 5-bit-per-channel histogram.
pub fn median_cut(pix: &[u8], n: usize) -> Vec<[u8; 3]> {
    #[derive(Clone, Copy, Default)]
    struct Bin {
        count: u64,
        sum: [u64; 3],  // sums of the exact colors in this bin
        coord: [u8; 3], // 5-bit coordinates
    }
    let mut bins = vec![Bin::default(); 1 << 15];
    for p in pix.as_chunks::<4>().0 {
        let key = (p[0] as usize >> 3) << 10 | (p[1] as usize >> 3) << 5 | p[2] as usize >> 3;
        let b = &mut bins[key];
        b.coord = [p[0] >> 3, p[1] >> 3, p[2] >> 3];
        b.count += 1;
        for (sum, &v) in b.sum.iter_mut().zip(p) {
            *sum += v as u64;
        }
    }
    let all: Vec<Bin> = bins.into_iter().filter(|b| b.count > 0).collect();
    // boxes are split along their widest channel at the weighted median
    let mut boxes = vec![all];
    while boxes.len() < n {
        let mut best: Option<(usize, u8, usize)> = None; // box, range, channel
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for c in 0..3 {
                let lo = b.iter().map(|e| e.coord[c]).min().unwrap();
                let hi = b.iter().map(|e| e.coord[c]).max().unwrap();
                if hi - lo > best.map_or(0, |b| b.1) {
                    best = Some((i, hi - lo, c));
                }
            }
        }
        let Some((i, _, c)) = best else { break };
        let mut b = std::mem::take(&mut boxes[i]);
        b.sort_by_key(|e| e.coord[c]);
        let total: u64 = b.iter().map(|e| e.count).sum();
        let (mut split, mut acc) = (1, 0);
        for (j, e) in b[..b.len() - 1].iter().enumerate() {
            acc += e.count;
            split = j + 1;
            if acc * 2 >= total {
                break;
            }
        }
        let rest = b.split_off(split);
        boxes[i] = b;
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let count: u64 = b.iter().map(|e| e.count).sum();
            std::array::from_fn(|k| (b.iter().map(|e| e.sum[k]).sum::<u64>() / count) as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{self, Scanline};
    use crate::rng::Rng;

    #[test]
    fn colors_from_the_image() {
        let pix = [[250, 10, 10, 255], [10, 250, 10, 255], [10, 10, 250, 255]].repeat(20);
        let p = Palette::from_image(pix.as_flattened(), 8);
        let mut rgb: Vec<_> = p.colors().iter().map(|c| (c.r, c.g, c.b)).collect();
        rgb.sort();
        assert_eq!(rgb, [(10, 10, 250), (10, 250, 10), (250, 10, 10)]);
        assert_eq!(Palette::from_image(pix.as_flattened(), 2).colors().len(), 2);
        assert_eq!(Palette::new(vec![]), None);
    }

    // The nearest palette color to the best color fits as well as the
    // best of drawing each one and scoring it, but for rounding.
    #[test]
    fn nearest_to_the_best_color_fits_best() {
        let mut rnd = Rng::new(1);
        let (w, h) = (16, 16);
        for _ in 0..300 {
            let mut image = || -> Vec<u8> { (0..w * h * 4).map(|_| rnd.intn(256) as u8).collect() };
            let (target, mut current) = (image(), image());
            current.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
            let n = 1 + rnd.intn(8) as usize;
            let colors = (0..n)
                .map(|_| Color {
                    r: rnd.intn(256),
                    g: rnd.intn(256),
                    b: rnd.intn(256),
                    a: 255,
                })
                .collect();
            let palette = Palette::new(colors).unwrap();
            let alpha = 1 + rnd.intn(255);
            let lines: Vec<Scanline> = (2..12)
                .map(|y| Scanline {
                    y,
                    x1: rnd.intn(8),
                    x2: 8 + rnd.intn(8),
                    alpha: 0xffff,
                })
                .collect();
            let error = |c: Color| {
                let mut im = current.clone();
                core::draw_lines(&mut im, w, c, &lines);
                core::difference_full(&target, &im, None)
            };
            let chosen =
                core::compute_color(&target, &current, w, &lines, alpha, Some(&palette), None);
            let best = palette
                .colors()
                .iter()
                .map(|&c| error(Color { a: alpha, ..c }))
                .fold(f64::INFINITY, f64::min);
            assert!(error(chosen) <= best * 1.001, "{} vs {best}", error(chosen));
        }
    }
}
