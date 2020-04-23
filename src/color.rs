use std::ops::{Index, IndexMut};
#[derive(Debug, Clone, Copy)]
pub struct Color {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}
// const BLACK: Color = Color { red: 0, green: 0, blue: 0, alpha: 0 };
impl Color {
    pub const BLACK: Color = Color {
        red: 0,
        green: 0,
        blue: 0,
        alpha: 0,
    };
    pub const WHITE: Color = Color {
        red: 255,
        green: 255,
        blue: 255,
        alpha: 0,
    };
    pub const RED: Color = Color {
        red: 255,
        green: 0,
        blue: 0,
        alpha: 0,
    };
    pub const ORANGE: Color = Color {
        red: 255,
        green: 127,
        blue: 0,
        alpha: 0,
    };
    pub const ORANGE_NORM: Color = Color {
        red: 170,
        green: 85,
        blue: 0,
        alpha: 0,
    };
    pub const YELLOW: Color = Color {
        red: 255,
        green: 255,
        blue: 0,
        alpha: 0,
    };
    pub const YELLOW_NORM: Color = Color {
        red: 170,
        green: 170,
        blue: 0,
        alpha: 0,
    };
    pub const GREEN: Color = Color {
        red: 0,
        green: 255,
        blue: 0,
        alpha: 0,
    };
    pub const BLUE: Color = Color {
        red: 0,
        green: 0,
        blue: 255,
        alpha: 0,
    };
    pub const MAGENTA: Color = Color {
        red: 255,
        green: 0,
        blue: 255,
        alpha: 0,
    };
    pub const MAGENTA_NORM: Color = Color {
        red: 170,
        green: 0,
        blue: 170,
        alpha: 0,
    };
    pub const PURPLE: Color = Color {
        red: 128,
        green: 0,
        blue: 128,
        alpha: 0,
    };
    #[inline]
    pub fn to_rgb(&self) -> [u8; 3] {
        [self.red, self.blue, self.green]
    }
    #[inline]
    pub fn to_rgba(&self) -> [u8; 4] {
        [self.red, self.blue, self.green, self.alpha]
    }
    #[inline]
    pub fn to_bgr(&self) -> [u8; 3] {
        [self.blue, self.green, self.red]
    }
    #[inline]
    pub fn to_bgra(&self) -> [u8; 4] {
        [self.blue, self.green, self.red, self.alpha]
    }
}

impl Default for Color {
    #[inline]
    fn default() -> Self {
        Color::BLACK
    }
}
pub struct ColorMap([Color; 256]);

impl Index<u8> for ColorMap {
    type Output = Color;
    fn index(&self, index: u8) -> &Self::Output {
        &self.0[index as usize]
    }
}
impl IndexMut<u8> for ColorMap {
    fn index_mut(&mut self, index: u8) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}

impl Default for ColorMap {
    fn default() -> Self {
        let mut cm = [Color::BLACK; 256];
        cm[1] = Color::RED;
        cm[2] = Color::ORANGE_NORM;
        cm[3] = Color::YELLOW_NORM;
        cm[4] = Color::GREEN;
        cm[5] = Color::BLUE;
        cm[6] = Color::MAGENTA_NORM;
        ColorMap(cm)
    }
}
