use crate::graphics::ScalarRange;

#[derive(PartialEq, Eq, Debug)]
pub struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}
impl Rect {
    pub fn new(x: i64, y: i64, w: i64, h: i64) -> Option<Rect> {
        if w < 0 || h < 0 {
            None
        } else {
            Some(Self { x, y, w, h })
        }
    }
    pub fn x(&self) -> i64 {
        self.x
    }
    pub fn y(&self) -> i64 {
        self.y
    }
    pub fn w(&self) -> i64 {
        self.w
    }
    pub fn h(&self) -> i64 {
        self.h
    }
    pub fn frame_ranges(&self) -> (ScalarRange, ScalarRange) {
        (
            ScalarRange::new(self.x, self.x + self.w).unwrap(),
            ScalarRange::new(self.y, self.y + self.h).unwrap(),
        )
    }
    pub fn intersection(&self, another: &Self) -> Option<Rect> {
        let (rx0, ry0) = self.frame_ranges();
        let (rx1, ry1) = another.frame_ranges();
        let rx = rx0.intersection(&rx1)?;
        let ry = ry0.intersection(&ry1)?;
        let x = rx.start();
        let w = rx.end() - rx.start();
        let y = ry.start();
        let h = ry.end() - ry.start();
        Some(Self { x, y, w, h })
    }
}

#[cfg(test)]
mod rect_tests {
    use super::Rect;

    #[test_case]
    fn creates_rect() {
        let r = Rect::new(0, 0, 0, 0).unwrap();
        assert_eq!(r.x(), 0);
        assert_eq!(r.y(), 0);
        assert_eq!(r.w(), 0);
        assert_eq!(r.h(), 0);

        let r = Rect::new(1, 2, 3, 4).unwrap();
        assert_eq!(r.x(), 1);
        assert_eq!(r.y(), 2);
        assert_eq!(r.w(), 3);
        assert_eq!(r.h(), 4);

        let r = Rect::new(-1, -2, 3, 4).unwrap();
        assert_eq!(r.x(), -1);
        assert_eq!(r.y(), -2);
        assert_eq!(r.w(), 3);
        assert_eq!(r.h(), 4);
    }
    #[test_case]
    fn fails_to_create_negative_sized_rect() {
        assert!(Rect::new(0, 0, -1, 0).is_none());
        assert!(Rect::new(0, 0, 0, -1).is_none());
        assert!(Rect::new(0, 0, -1, -1).is_none());
    }
    #[test_case]
    fn calc_intersection() {
        let r1 = Rect::new(0, 0, 1, 1).unwrap();
        let self_intersect = r1.intersection(&r1).unwrap();
        assert_eq!(self_intersect, r1);
    }
}
