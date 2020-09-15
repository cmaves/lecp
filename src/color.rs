/// Defines the `Color` and `ColorMap` that are used to set colors on the 
/// receiver.

use std::ops::{Deref, DerefMut, Mul, MulAssign};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}
impl MulAssign<f32> for Color {
    fn mul_assign(&mut self, rhs: f32) {
        *self = *self * rhs;
    }
}
impl MulAssign<f64> for Color {
    fn mul_assign(&mut self, rhs: f64) {
        *self = *self * rhs;
    }
}
impl Mul<f32> for Color {
    type Output = Self;
    fn mul(mut self, rhs: f32) -> Self::Output {
        self.red = (self.red as f32 * rhs).round().min(255.0) as u8;
        self.blue = (self.blue as f32 * rhs).round().min(255.0) as u8;
        self.green = (self.green as f32 * rhs).round().min(255.0) as u8;
        self
    }
}
impl Mul<f64> for Color {
    type Output = Self;
    fn mul(mut self, rhs: f64) -> Self::Output {
        self.red = (self.red as f64 * rhs).round().min(255.0) as u8;
        self.blue = (self.blue as f64 * rhs).round().min(255.0) as u8;
        self.green = (self.green as f64 * rhs).round().min(255.0) as u8;
        self
    }
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
    pub const WHITE_NORM: Color = Color {
        red: 85,
        green: 85,
        blue: 85,
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
        red: 128,
        green: 128,
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
        red: 128,
        green: 0,
        blue: 128,
        alpha: 0,
    };
    pub const PURPLE: Color = Color {
        red: 128,
        green: 0,
        blue: 128,
        alpha: 0,
    };
    #[inline]
    pub fn from_bgra(color: [u8; 4]) -> Self {
        Color {
            blue: color[0],
            green: color[1],
            red: color[2],
            alpha: color[3],
        }
    }
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

impl Deref for ColorMap {
    type Target = [Color; 256];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for ColorMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
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
