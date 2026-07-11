/// Fill an canvas with a sliding gradient rectangles
pub fn fill(canvas: &mut [u8], width: u32, height: u32, time_ms: u32) {
    let t = (time_ms / 60) as u8;
    let mut i = 0;
    for y in 0..height {
        let g = (y as u8).wrapping_add(t);
        for x in 0..width {
            let r = (x as u8).wrapping_add(t);

            // alpha(255), red(r), green(g), blue(128)
            let color: u32 = 0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | 0x80;
            canvas[i..i + 4].copy_from_slice(&color.to_le_bytes());
            i += 4;
        }
    }
}
