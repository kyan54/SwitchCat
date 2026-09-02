pub const ICON_SIZE: u32 = 32;
pub const FRAME_COUNT: usize = 8;

const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
const OUTLINE: [u8; 4] = [74, 52, 58, 255];
const ORANGE: [u8; 4] = [250, 157, 62, 255];
const CREAM: [u8; 4] = [255, 231, 183, 255];
const TEAL: [u8; 4] = [33, 177, 166, 255];

pub fn running_cat_frame(phase: usize) -> Vec<u8> {
    let mut canvas = vec![0; (ICON_SIZE * ICON_SIZE * 4) as usize];
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&TRANSPARENT);
    }

    let phase = phase % FRAME_COUNT;
    let bob = [1, 0, 0, 1, 2, 1, 0, 0][phase];
    let leg_a = [1, 2, 3, 2, 0, -1, -2, -1][phase];
    let leg_b = [-2, -1, 0, 2, 3, 2, 1, -1][phase];

    // Tail, drawn first so the body naturally overlaps it.
    let tail_tip_y = 9 + [0, -1, -2, -1, 0, 1, 2, 1][phase];
    thick_line(&mut canvas, 10, 18 + bob, 5, 14 + bob, 4, OUTLINE);
    thick_line(&mut canvas, 5, 14 + bob, 3, tail_tip_y, 4, OUTLINE);
    thick_line(&mut canvas, 10, 18 + bob, 5, 14 + bob, 2, ORANGE);
    thick_line(&mut canvas, 5, 14 + bob, 3, tail_tip_y, 2, ORANGE);

    // Back and front legs alternate to create a continuous running gait.
    leg(&mut canvas, 12, 20 + bob, leg_a, false);
    leg(&mut canvas, 19, 20 + bob, leg_b, false);

    ellipse(&mut canvas, 8, 12 + bob, 24, 24 + bob, OUTLINE);
    ellipse(&mut canvas, 9, 13 + bob, 23, 23 + bob, ORANGE);
    ellipse(&mut canvas, 11, 17 + bob, 21, 23 + bob, CREAM);

    leg(&mut canvas, 14, 20 + bob, leg_b, true);
    leg(&mut canvas, 22, 19 + bob, leg_a, true);

    // Head and ears.
    triangle(&mut canvas, (20, 12 + bob), (22, 6 + bob), (25, 12 + bob), OUTLINE);
    triangle(&mut canvas, (25, 11 + bob), (28, 6 + bob), (30, 13 + bob), OUTLINE);
    triangle(&mut canvas, (22, 11 + bob), (23, 8 + bob), (24, 11 + bob), ORANGE);
    triangle(&mut canvas, (27, 11 + bob), (28, 8 + bob), (29, 12 + bob), ORANGE);
    ellipse(&mut canvas, 19, 9 + bob, 30, 20 + bob, OUTLINE);
    ellipse(&mut canvas, 20, 10 + bob, 29, 19 + bob, ORANGE);
    ellipse(&mut canvas, 23, 15 + bob, 29, 19 + bob, CREAM);

    // Collar, eye, nose, and two tiny whisker pixels survive at tray size.
    rect(&mut canvas, 20, 18 + bob, 25, 19 + bob, TEAL);
    rect(&mut canvas, 25, 12 + bob, 26, 13 + bob, OUTLINE);
    set_pixel(&mut canvas, 29, 16 + bob, OUTLINE);
    set_pixel(&mut canvas, 30, 15 + bob, OUTLINE);
    set_pixel(&mut canvas, 30, 17 + bob, OUTLINE);

    canvas
}

fn leg(canvas: &mut [u8], x: i32, y: i32, stride: i32, foreground: bool) {
    let color = if foreground { ORANGE } else { [216, 119, 52, 255] };
    thick_line(canvas, x, y, x + stride, y + 6, 4, OUTLINE);
    thick_line(canvas, x, y, x + stride, y + 6, 2, color);
    thick_line(canvas, x + stride, y + 6, x + stride + 3, y + 6, 3, OUTLINE);
    thick_line(canvas, x + stride, y + 6, x + stride + 3, y + 6, 1, color);
}

fn rect(canvas: &mut [u8], left: i32, top: i32, right: i32, bottom: i32, color: [u8; 4]) {
    for y in top..=bottom {
        for x in left..=right {
            set_pixel(canvas, x, y, color);
        }
    }
}

fn ellipse(
    canvas: &mut [u8],
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color: [u8; 4],
) {
    let center_x = (left + right) as f32 / 2.0;
    let center_y = (top + bottom) as f32 / 2.0;
    let radius_x = (right - left).max(1) as f32 / 2.0;
    let radius_y = (bottom - top).max(1) as f32 / 2.0;
    for y in top..=bottom {
        for x in left..=right {
            let dx = (x as f32 - center_x) / radius_x;
            let dy = (y as f32 - center_y) / radius_y;
            if dx * dx + dy * dy <= 1.0 {
                set_pixel(canvas, x, y, color);
            }
        }
    }
}

fn triangle(
    canvas: &mut [u8],
    a: (i32, i32),
    b: (i32, i32),
    c: (i32, i32),
    color: [u8; 4],
) {
    let min_x = a.0.min(b.0).min(c.0);
    let max_x = a.0.max(b.0).max(c.0);
    let min_y = a.1.min(b.1).min(c.1);
    let max_y = a.1.max(b.1).max(c.1);
    let area = edge(a, b, c);
    if area == 0 {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = (x, y);
            let w0 = edge(b, c, point);
            let w1 = edge(c, a, point);
            let w2 = edge(a, b, point);
            if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                set_pixel(canvas, x, y, color);
            }
        }
    }
}

fn edge(a: (i32, i32), b: (i32, i32), point: (i32, i32)) -> i32 {
    (point.0 - a.0) * (b.1 - a.1) - (point.1 - a.1) * (b.0 - a.0)
}

fn thick_line(
    canvas: &mut [u8],
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: [u8; 4],
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    let radius = thickness / 2;

    loop {
        for offset_y in -radius..=radius {
            for offset_x in -radius..=radius {
                if offset_x * offset_x + offset_y * offset_y <= radius * radius + 1 {
                    set_pixel(canvas, x0 + offset_x, y0 + offset_y, color);
                }
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice_error = 2 * error;
        if twice_error >= dy {
            error += dy;
            x0 += sx;
        }
        if twice_error <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn set_pixel(canvas: &mut [u8], x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= ICON_SIZE as i32 || y >= ICON_SIZE as i32 {
        return;
    }
    let index = ((y as u32 * ICON_SIZE + x as u32) * 4) as usize;
    canvas[index..index + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_has_the_expected_rgba_size() {
        for phase in 0..FRAME_COUNT {
            let frame = running_cat_frame(phase);
            assert_eq!(frame.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
            assert!(frame.chunks_exact(4).any(|pixel| pixel[3] != 0));
        }
    }
}
