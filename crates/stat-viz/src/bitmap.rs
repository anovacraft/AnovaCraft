//! Bitmaps inside figures: an image plot's picture is a grid of coloured
//! cells, placed in a page rectangle and drawn cell for cell (no smoothing).
//! The figure writers embed it in their own way — PNG data in SVG, an image
//! XObject in PDF, an inline `image` in EPS — so these are the encodings they
//! share: PNG files, Base64 (SVG data URIs, figure drawings), ASCII85 (PDF and
//! PostScript streams) and zlib-compressed bytes (PDF `FlateDecode`).

/// A picture of `width` × `height` cells as straight (not premultiplied)
/// RGBA bytes, row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bitmap {
    /// Cells across.
    pub width: u32,
    /// Cells down.
    pub height: u32,
    /// `4 × width × height` bytes: red, green, blue, alpha of each cell.
    pub rgba: Vec<u8>,
}

impl Bitmap {
    /// Whether the bitmap has cells and as many bytes as they need.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.rgba.len() == 4 * self.width as usize * self.height as usize
    }

    /// Whether any cell is not fully opaque.
    #[must_use]
    pub fn has_alpha(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] != 255)
    }

    /// The bitmap as a PNG file (8-bit RGBA).
    #[must_use]
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("PNG header into memory");
            writer.write_image_data(&self.rgba).expect("PNG data into memory");
        }
        out
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in Base64 (RFC 4648, with `=` padding, no line breaks).
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(BASE64[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes of Base64 text written by [`base64_encode`]; `None` when the
/// text is not Base64.
#[must_use]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = BASE64.iter().position(|&b| b == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// `bytes` in ASCII85 (the PostScript and PDF `ASCII85Decode` form): groups
/// of four bytes as five characters `!`–`u`, an all-zero group as `z`, lines
/// of at most 75 characters, ending with `~>`.
#[must_use]
pub fn ascii85_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 5 / 4 + bytes.len() / 60 + 4);
    let mut line = 0;
    let mut push = |out: &mut String, c: char| {
        out.push(c);
        line += 1;
        if line == 75 {
            out.push('\n');
            line = 0;
        }
    };
    for chunk in bytes.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        let mut n = u32::from_be_bytes(b);
        if chunk.len() == 4 && n == 0 {
            push(&mut out, 'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (n % 85) as u8;
            n /= 85;
        }
        for &d in &digits[..chunk.len() + 1] {
            push(&mut out, char::from(b'!' + d));
        }
    }
    out.push_str("~>");
    out
}

/// `bytes` compressed in the zlib format (what PDF's `FlateDecode` reads).
#[must_use]
pub fn zlib_compress(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_and_matches_the_rfc_examples() {
        // RFC 4648 §10 test vectors.
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), coded);
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        assert!(base64_decode("ab$d").is_none());
    }

    /// Decode ASCII85 as a PostScript interpreter would.
    fn ascii85_decode(text: &str) -> Vec<u8> {
        let body = text.trim_end_matches("~>");
        let mut out = Vec::new();
        let mut group: Vec<u32> = Vec::new();
        for c in body.chars().filter(|c| !c.is_whitespace()) {
            if c == 'z' {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            group.push(c as u32 - 33);
            if group.len() == 5 {
                let n = group.iter().fold(0u32, |a, &d| a * 85 + d);
                out.extend_from_slice(&n.to_be_bytes());
                group.clear();
            }
        }
        if !group.is_empty() {
            let k = group.len();
            group.resize(5, 84);
            let n = group.iter().fold(0u32, |a, &d| a * 85 + d);
            out.extend_from_slice(&n.to_be_bytes()[..k - 1]);
        }
        out
    }

    #[test]
    fn ascii85_round_trips_with_zero_groups_and_short_ends() {
        // The classic example from the PostScript Language Reference.
        assert_eq!(ascii85_encode(b"Man "), "9jqo^~>");
        for n in [0usize, 1, 2, 3, 4, 5, 7, 8, 200] {
            let bytes: Vec<u8> = (0..n).map(|i| if i % 9 < 4 { 0 } else { (i * 37) as u8 }).collect();
            let coded = ascii85_encode(&bytes);
            assert!(coded.lines().all(|l| l.len() <= 75), "{coded}");
            assert_eq!(ascii85_decode(&coded), bytes, "{n} bytes");
        }
    }

    #[test]
    fn zlib_output_inflates_back() {
        let bytes: Vec<u8> = (0..5000).map(|i| (i % 251) as u8).collect();
        let packed = zlib_compress(&bytes);
        assert_eq!(packed[0], 0x78, "zlib header");
        assert_eq!(miniz_oxide::inflate::decompress_to_vec_zlib(&packed).unwrap(), bytes);
    }

    #[test]
    fn bitmaps_know_their_size_and_alpha() {
        let b = Bitmap { width: 2, height: 1, rgba: vec![1, 2, 3, 255, 4, 5, 6, 255] };
        assert!(b.is_valid() && !b.has_alpha());
        let png = b.to_png();
        assert_eq!(&png[1..4], b"PNG");
        let clear = Bitmap { rgba: vec![1, 2, 3, 0, 4, 5, 6, 255], ..b.clone() };
        assert!(clear.has_alpha());
        assert!(!Bitmap { width: 3, ..b }.is_valid());
    }
}
