//! PlxNative's shared full-width billboard spring, independent of content identity.
//! Callers retain their outgoing content until step reports settlement.
use super::{consts::SCR_W, Spring};
pub(crate) const FLIP_COOLDOWN: f32 = 0.35;
const STIFFNESS: f32 = 130.;
const REST_PIXELS: f32 = 0.5;

pub(crate) fn begin(slide: &mut Spring) {
    slide.jump(0.);
}
pub(crate) fn offsets(slide: &Spring, direction: f32) -> (f32, f32) {
    (
        -direction * slide.pos * SCR_W,
        direction * (1. - slide.pos) * SCR_W,
    )
}
pub(crate) fn step(slide: &mut Spring, dt: f32) -> bool {
    slide.step(1., STIFFNESS, dt);
    if (1. - slide.pos).abs() * SCR_W < REST_PIXELS {
        slide.jump(1.);
        true
    } else {
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_motion_slides_in_both_directions_and_settles() {
        for direction in [-1., 1.] {
            let mut slide = Spring::at(1.);
            begin(&mut slide);
            assert_eq!(offsets(&slide, direction), (0., direction * SCR_W));
            assert!(!step(&mut slide, 1. / 60.));
            let (out, incoming) = offsets(&slide, direction);
            assert!(out * direction < 0. && incoming * direction > 0.);
            for _ in 0..240 {
                if step(&mut slide, 1. / 60.) {
                    break;
                }
            }
            assert_eq!(offsets(&slide, direction), (-direction * SCR_W, 0.));
        }
    }
}
