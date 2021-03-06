//! An SDL2-based simulator for the EPD.
//!
//! Strongly derived from [the
//! simulator](https://github.com/jamwaffles/embedded-graphics/tree/master/simulator)
//! provided with the
//! [embedded-graphics](https://crates.io/crates/embedded-graphics) crate.
//!
//! TODO: the From<u8> behavior of this could be changed to exactly match that
//! of waveshare-epd, in which only 0 and 1 are valid inputs and (IIRC) 1 is
//! white.

// To minimize differences with upstream, we keep in a few features that we
// don't use, so:
#![allow(unused)]

use embedded_graphics::{drawable::Pixel, prelude::*, Drawing};
use sdl2::{event::Event, keyboard::Keycode, pixels::Color, rect::Rect, render};
use std::{io::Error, thread, time::Duration};

use super::DisplayBackend;

// Begin stuff that's basically copy/pasted from
// embedded-graphics/simulator/src/lib.rs

#[derive(Clone, Copy, PartialEq)]
pub struct SimPixelColor(pub bool);

impl PixelColor for SimPixelColor {}

impl From<u8> for SimPixelColor {
    fn from(other: u8) -> Self {
        SimPixelColor(other != 0)
    }
}

impl From<u16> for SimPixelColor {
    fn from(other: u16) -> Self {
        SimPixelColor(other != 0)
    }
}

pub struct Display {
    width: usize,
    height: usize,
    scale: usize,
    pixel_spacing: usize,
    background_color: Color,
    pixel_color: Color,
    pixels: Box<[SimPixelColor]>,
    canvas: render::Canvas<sdl2::video::Window>,
    event_pump: sdl2::EventPump,
}

impl Display {
    /// XXX modified for rc-stickynote
    pub fn run_once(&mut self) -> bool {
        let mut should_exit = false;

        // Handle events
        for event in self.event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    should_exit = true;
                }
                _ => {}
            }
        }

        self.canvas.set_draw_color(self.background_color);
        self.canvas.clear();

        self.canvas.set_draw_color(self.pixel_color);
        let pitch = self.scale + self.pixel_spacing;
        for (index, value) in self.pixels.iter().enumerate() {
            if *value == SimPixelColor(true) {
                let x = (index % self.width * pitch) as i32;
                let y = (index / self.width * pitch) as i32;
                let r = Rect::new(x, y, self.scale as u32, self.scale as u32);
                self.canvas.fill_rect(r).unwrap();
            }
        }

        self.canvas.present();
        should_exit
    }

    /// XXX new method for rc-stickynote:
    pub fn fill(&mut self, color: SimPixelColor) {
        for p in self.pixels.iter_mut() {
            *p = color;
        }
    }
}

impl Drawing<SimPixelColor> for Display {
    fn draw<T>(&mut self, item_pixels: T)
    where
        T: IntoIterator<Item = Pixel<SimPixelColor>>,
    {
        for Pixel(coord, color) in item_pixels {
            let x = coord[0] as usize;
            let y = coord[1] as usize;

            if x >= self.width || y >= self.height {
                continue;
            }

            self.pixels[y * self.width + x] = color;
        }
    }
}

pub enum DisplayTheme {
    LcdWhite,
    LcdGreen,
    LcdBlue,
    OledWhite,
    OledBlue,
}

pub struct DisplayBuilder {
    width: usize,
    height: usize,
    scale: usize,
    pixel_spacing: usize,
    background_color: Color,
    pixel_color: Color,
}

impl DisplayBuilder {
    pub fn new() -> Self {
        Self {
            width: 256,
            height: 256,
            scale: 1,
            pixel_spacing: 0,
            background_color: Color::RGB(255, 255, 255),
            pixel_color: Color::RGB(0, 0, 0),
        }
    }

    pub fn size(&mut self, width: usize, height: usize) -> &mut Self {
        if width == 0 || height == 0 {
            panic!("with and height must be >= 0");
        }

        self.width = width;
        self.height = height;

        self
    }

    pub fn scale(&mut self, scale: usize) -> &mut Self {
        if scale == 0 {
            panic!("scale must be >= 0");
        }

        self.scale = scale;

        self
    }

    pub fn background_color(&mut self, r: u8, g: u8, b: u8) -> &mut Self {
        self.background_color = Color::RGB(r, g, b);

        self
    }

    pub fn pixel_color(&mut self, r: u8, g: u8, b: u8) -> &mut Self {
        self.pixel_color = Color::RGB(r, g, b);

        self
    }

    pub fn theme(&mut self, theme: DisplayTheme) -> &mut Self {
        match theme {
            DisplayTheme::LcdWhite => {
                self.background_color(245, 245, 245);
                self.pixel_color(32, 32, 32);
            }
            DisplayTheme::LcdGreen => {
                self.background_color(120, 185, 50);
                self.pixel_color(32, 32, 32);
            }
            DisplayTheme::LcdBlue => {
                self.background_color(70, 80, 230);
                self.pixel_color(230, 230, 255);
            }
            DisplayTheme::OledBlue => {
                self.background_color(0, 20, 40);
                self.pixel_color(0, 210, 255);
            }
            DisplayTheme::OledWhite => {
                self.background_color(20, 20, 20);
                self.pixel_color(255, 255, 255);
            }
        }

        self.scale(3);
        self.pixel_spacing(1);

        self
    }

    pub fn pixel_spacing(&mut self, pixel_spacing: usize) -> &mut Self {
        self.pixel_spacing = pixel_spacing;

        self
    }

    pub fn build(&self) -> Display {
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window_width = self.width * self.scale + (self.width - 1) * self.pixel_spacing;
        let window_height = self.height * self.scale + (self.height - 1) * self.pixel_spacing;

        let window = video_subsystem
            .window(
                "graphics-emulator",
                window_width as u32,
                window_height as u32,
            )
            .position_centered()
            .build()
            .unwrap();

        let pixels = vec![SimPixelColor(false); self.width * self.height];
        let canvas = window.into_canvas().build().unwrap();
        let event_pump = sdl_context.event_pump().unwrap();

        Display {
            width: self.width,
            height: self.height,
            scale: self.scale,
            pixel_spacing: self.pixel_spacing,
            background_color: self.background_color,
            pixel_color: self.pixel_color,
            pixels: pixels.into_boxed_slice(),
            canvas,
            event_pump,
        }
    }
}

// Here's some novelty to make the above pluggable with my code.

pub struct SimulatorBackend {
    display: Display,
}

impl DisplayBackend for SimulatorBackend {
    type Color = SimPixelColor;
    type Buffer = Display;

    const BLACK: SimPixelColor = SimPixelColor(true);
    const WHITE: SimPixelColor = SimPixelColor(false);

    fn open() -> Result<Self, Error> {
        // Make the size the same as the Waveshare 7in5 that I have.
        let display = DisplayBuilder::new().size(384, 640).build();

        Ok(SimulatorBackend { display })
    }

    fn get_buffer_mut(&mut self) -> &mut Self::Buffer {
        &mut self.display
    }

    fn clear_buffer(&mut self, color: Self::Color) -> Result<(), Error> {
        self.display.fill(color);
        Ok(())
    }

    fn show_buffer(&mut self) -> Result<(), Error> {
        println!("*** hit Escape when you're done looking at this image ***");

        loop {
            let end = self.display.run_once();

            if end {
                break;
            }

            thread::sleep(Duration::from_millis(200));
        }

        println!("*** unblocking thread ***");
        Ok(())
    }

    fn clear_display(&mut self) -> Result<(), Error> {
        println!("*** simulator no-op: clear_display() ***");
        Ok(())
    }

    fn sleep_device(&mut self) -> Result<(), Error> {
        println!("*** simulator no-op: sleep_device() ***");
        Ok(())
    }

    fn wake_up_device(&mut self) -> Result<(), Error> {
        println!("*** simulator no-op: wake_up_device() ***");
        Ok(())
    }
}
