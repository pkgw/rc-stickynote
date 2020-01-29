//! Rendering text with TTF font support.
//!
//! Sadly there is an impedance mismatch between the rusttype and
//! embedded-graphics APIs: rusttype wants to be given a closure that it will
//! call with (x, y, value), whereas embedded-graphics wants an iterator of
//! (x, y, value). So we have to buffer.

use embedded_graphics::{pixelcolor::PixelColor, prelude::*};
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::marker::PhantomData;

/// A convenience extension trait to help with rasterizing a rusttype font
/// into an embedded-graphics Drawing.
pub trait DrawFontExt {
    /// Rasterize the given text at the given height into a layout buffer.
    fn rasterize(&self, text: &str, height: f32) -> Layout;
}

impl<'a> DrawFontExt for Font<'a> {
    fn rasterize(&self, text: &str, float_height: f32) -> Layout {
        let height = float_height.ceil() as usize;

        let scale = Scale {
            x: float_height,
            y: float_height,
        };

        // This stuff copied from the rusttype sample.rs file:
        let v_metrics = self.v_metrics(scale);
        let offset = point(0.0, v_metrics.ascent);
        let glyphs: Vec<PositionedGlyph<'_>> = self.layout(text, scale, offset).collect();
        let width = glyphs
            .iter()
            .rev()
            .map(|g| g.position().x as f32 + g.unpositioned().h_metrics().advance_width)
            .next()
            .unwrap_or(0.0)
            .ceil() as usize;

        let mut buf: Vec<u8> = vec![0u8.into(); width * height];

        for g in glyphs {
            if let Some(bb) = g.pixel_bounding_box() {
                g.draw(|x, y, v| {
                    let x = x as i32 + bb.min.x;
                    let y = y as i32 + bb.min.y;

                    // There's still a possibility that the glyph clips the boundaries of the bitmap
                    if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                        let x = x as usize;
                        let y = y as usize;
                        buf[x + y * width] = (v * 255.0) as u8;
                    }
                })
            }
        }

        Layout { buf, width, height }
    }
}

/// A buffered rasterization of a bit of text.
#[derive(Clone, Debug)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    buf: Vec<u8>,
}

impl Layout {
    /// Represent this rasterization as a pixel iterator suitable for
    /// consumption by `embedded_graphics::Drawing::draw()`.
    pub fn draw_at<'a, C: PixelColor>(&'a self, x0: usize, y0: usize) -> LayoutPixelIter<'a, C> {
        LayoutPixelIter {
            layout: self,
            x0,
            y0,
            ix: 0,
            iy: 0,
            _phantom: PhantomData,
        }
    }
}

/// An iterator over pixels in a Layout.
#[derive(Debug)]
pub struct LayoutPixelIter<'a, C> {
    layout: &'a Layout,
    x0: usize,
    y0: usize,
    ix: usize,
    iy: usize,
    _phantom: PhantomData<C>,
}

impl<'a, C: PixelColor> Iterator for LayoutPixelIter<'a, C> {
    type Item = Pixel<C>;

    fn next(&mut self) -> Option<Pixel<C>> {
        if self.iy >= self.layout.height {
            return None;
        }

        let rx = (self.x0 + self.ix) as u32;
        let ry = (self.y0 + self.iy) as u32;
        let rc = self.layout.buf[self.ix + self.iy * self.layout.width].into();

        self.ix += 1;

        if self.ix >= self.layout.width {
            self.ix = 0;
            self.iy += 1;
        }

        Some(Pixel(UnsignedCoord(rx, ry), rc))
    }
}
