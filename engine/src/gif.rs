//! Animated GIFs of shapes being added: a median-cut palette from the finished image, and only the changed
//! region of each frame, with unchanged pixels transparent.

use crate::model::Model;
use crate::render::Animator;

/// Encodes the animation of the first count shapes, fitting within
/// max_side x max_side. delay is the time between frames and last_delay the
/// time the finished image is held, both in hundredths of a second.
pub fn encode(
    model: &Model,
    count: usize,
    max_side: usize,
    frames: usize,
    delay: u16,
    last_delay: u16,
) -> Vec<u8> {
    let counts = model.animation_counts(frames.max(2), count);
    let mut a = Animator::new(model, max_side, false);
    let (width, height) = (a.width, a.height);

    // palette from the finished image; the index after it is transparent
    let final_frame = Animator::new(model, max_side, false)
        .frame(model, count)
        .to_vec();
    let pal = quantize(&final_frame, 255);
    let transparent = pal.len() as u8;
    let lookup = PaletteLookup::new(&pal);

    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.extend_from_slice(&[0xf7, 0, 0]); // 256-color global table
    for i in 0..256 {
        out.extend_from_slice(&pal.get(i).copied().unwrap_or([0, 0, 0]));
    }
    // loop forever
    out.extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00");

    struct Frame {
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        pix: Vec<u8>,
        delay: u16,
    }
    let mut list: Vec<Frame> = vec![];
    let mut prev = vec![0u8; width * height];
    let mut cur = vec![0u8; width * height];
    for (k, &count) in counts.iter().enumerate() {
        if k > 0 && count == counts[k - 1] {
            // nothing new to show: lengthen the previous frame instead
            list.last_mut().unwrap().delay += delay;
            continue;
        }
        let im = a.frame(model, count);
        for (c, p) in cur.iter_mut().zip(im.as_chunks::<4>().0) {
            *c = lookup.index(p[0], p[1], p[2]);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, width, height);
        if k > 0 {
            let Some(r) = changed_rect(&prev, &cur, width, height) else {
                list.last_mut().unwrap().delay += delay;
                continue;
            };
            (x0, y0, x1, y1) = r;
        }
        let mut pix = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * width + x;
                pix.push(if k > 0 && cur[i] == prev[i] {
                    transparent
                } else {
                    cur[i]
                });
            }
        }
        list.push(Frame {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
            pix,
            delay,
        });
        std::mem::swap(&mut prev, &mut cur);
    }
    list.last_mut().unwrap().delay = last_delay;

    for f in &list {
        // graphic control: no disposal, delay, transparent index
        out.extend_from_slice(&[0x21, 0xf9, 0x04, 0x05]);
        out.extend_from_slice(&f.delay.to_le_bytes());
        out.extend_from_slice(&[transparent, 0]);
        out.push(0x2c);
        for v in [f.x, f.y, f.w, f.h] {
            out.extend_from_slice(&(v as u16).to_le_bytes());
        }
        out.push(0);
        lzw(&f.pix, &mut out);
    }
    out.push(0x3b);
    out
}

/// The bounding box (x0, y0, x1, y1), exclusive, of the pixels that differ.
fn changed_rect(
    a: &[u8],
    b: &[u8],
    width: usize,
    height: usize,
) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (width, height, 0, 0);
    let mut any = false;
    for y in 0..height {
        for x in 0..width {
            if a[y * width + x] != b[y * width + x] {
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                y0 = y0.min(y);
                y1 = y1.max(y + 1);
                any = true;
            }
        }
    }
    any.then_some((x0, y0, x1, y1))
}

/// Builds a palette of at most n colors for RGBA pixels using median cut
/// over a 5-bit-per-channel histogram.
fn quantize(pix: &[u8], n: usize) -> Vec<[u8; 3]> {
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

/// Maps colors to their nearest palette index through a 5-bit-per-channel
/// table, which is much faster than searching per pixel.
struct PaletteLookup {
    table: Vec<u8>,
}

impl PaletteLookup {
    fn new(pal: &[[u8; 3]]) -> PaletteLookup {
        let table = (0..1 << 15)
            .map(|key: usize| {
                let r = ((key >> 10) << 3 | 4) as i32;
                let g = ((key >> 5 & 31) << 3 | 4) as i32;
                let b = ((key & 31) << 3 | 4) as i32;
                let dist = |c: &[u8; 3]| {
                    let (dr, dg, db) = (r - c[0] as i32, g - c[1] as i32, b - c[2] as i32);
                    2 * dr * dr + 4 * dg * dg + 3 * db * db
                };
                (0..pal.len()).min_by_key(|&i| dist(&pal[i])).unwrap_or(0) as u8
            })
            .collect();
        PaletteLookup { table }
    }

    fn index(&self, r: u8, g: u8, b: u8) -> u8 {
        self.table[(r as usize >> 3) << 10 | (g as usize >> 3) << 5 | b as usize >> 3]
    }
}

/// Writes 8-bit pixels as GIF image data: the minimum code size, then LZW
/// codes in sub-blocks of up to 255 bytes. The code-width and reset rules
/// follow Go's compress/lzw, which image/gif uses.
fn lzw(pix: &[u8], out: &mut Vec<u8>) {
    const LIT: u32 = 8;
    const CLEAR: u32 = 1 << LIT;
    const EOF: u32 = CLEAR + 1;
    const MAX_CODE: u32 = 4095;
    out.push(LIT as u8);
    let mut data = Vec::new();
    let (mut bits, mut nbits) = (0u32, 0u32);
    let mut width = LIT + 1;
    let mut write = |code: u32, width: u32, data: &mut Vec<u8>| {
        bits |= code << nbits;
        nbits += width;
        while nbits >= 8 {
            data.push(bits as u8);
            bits >>= 8;
            nbits -= 8;
        }
    };
    // table[prefix * 256 + byte] = code, valid when tagged with the current
    // generation, so that resetting is just a new generation
    let mut table = vec![0u32; 4096 * 256];
    let mut generation = 1u32;
    let mut hi = EOF;
    let mut overflow = CLEAR << 1;
    write(CLEAR, width, &mut data);
    if let Some((&first, rest)) = pix.split_first() {
        let mut code = first as u32;
        for &b in rest {
            let key = (code << 8 | b as u32) as usize;
            let entry = table[key];
            if entry >> 12 == generation {
                code = entry & 0xfff;
                continue;
            }
            write(code, width, &mut data);
            code = b as u32;
            hi += 1;
            if hi == overflow {
                width += 1;
                overflow <<= 1;
            }
            if hi == MAX_CODE {
                write(CLEAR, width, &mut data);
                width = LIT + 1;
                hi = EOF;
                overflow = CLEAR << 1;
                generation += 1;
                continue;
            }
            table[key] = generation << 12 | hi;
        }
        write(code, width, &mut data);
        hi += 1;
        if hi == overflow {
            width += 1;
        }
        if hi == MAX_CODE {
            write(CLEAR, width, &mut data);
            width = LIT + 1;
        }
    }
    write(EOF, width, &mut data);
    if nbits > 0 {
        data.push(bits as u8);
    }
    for chunk in data.chunks(255) {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    }
    out.push(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straightforward GIF LZW decoder, to check the encoder.
    fn unlzw(data: &[u8]) -> Vec<u8> {
        let lit = data[0] as u32;
        let mut bytes = vec![];
        let mut i = 1;
        while data[i] != 0 {
            let n = data[i] as usize;
            bytes.extend_from_slice(&data[i + 1..i + 1 + n]);
            i += n + 1;
        }
        let clear = 1u32 << lit;
        let mut dict: Vec<Vec<u8>> = vec![];
        let reset = |dict: &mut Vec<Vec<u8>>| {
            dict.clear();
            dict.extend((0..clear + 2).map(|c| vec![c as u8]));
        };
        reset(&mut dict);
        let (mut width, mut pos) = (lit + 1, 0usize);
        let mut out = vec![];
        let mut prev: Option<Vec<u8>> = None;
        loop {
            let mut code = 0u32;
            for b in 0..width {
                let bit = (bytes[(pos + b as usize) / 8] >> ((pos + b as usize) % 8)) & 1;
                code |= (bit as u32) << b;
            }
            pos += width as usize;
            if code == clear {
                reset(&mut dict);
                width = lit + 1;
                prev = None;
                continue;
            }
            if code == clear + 1 {
                return out;
            }
            let entry = if (code as usize) < dict.len() {
                dict[code as usize].clone()
            } else {
                let mut e = prev.clone().unwrap();
                e.push(e[0]);
                e
            };
            out.extend_from_slice(&entry);
            if let Some(mut p) = prev {
                p.push(entry[0]);
                dict.push(p);
            }
            if dict.len() as u32 == (1 << width) && width < 12 {
                width += 1;
            }
            prev = Some(entry);
        }
    }

    #[test]
    fn lzw_round_trip() {
        let mut rnd = crate::rng::Rng::new(9);
        for len in [1usize, 2, 300, 5000, 200_000] {
            // runs and noise, enough to fill the table and reset it
            let pix: Vec<u8> = (0..len)
                .map(|i| {
                    if i % 1000 < 500 {
                        (i / 37) as u8
                    } else {
                        rnd.intn(256) as u8
                    }
                })
                .collect();
            let mut out = vec![];
            lzw(&pix, &mut out);
            assert_eq!(unlzw(&out), pix, "length {len}");
        }
    }
}
