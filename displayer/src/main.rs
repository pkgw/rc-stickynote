//! The program that renders information to the e-Print Display. (Or a
//! simulated version thereof.)

use embedded_graphics::{
    coord::Coord,
    fonts::{Font12x16, Font6x8},
    prelude::*,
    primitives::{Circle, Line},
    Drawing,
};
use std::io::Error;

#[cfg(feature = "waveshare")]
mod epd7in5;
#[cfg(feature = "waveshare")]
use epd7in5::EPD7in5Backend as Backend;

#[cfg(feature = "simulator")]
mod simulator;
#[cfg(feature = "simulator")]
use simulator::SimulatorBackend as Backend;

trait DisplayBackend: Sized {
    type Color: embedded_graphics::pixelcolor::PixelColor;
    type Buffer: Drawing<Self::Color>;

    const BLACK: Self::Color;
    const WHITE: Self::Color;

    fn open() -> Result<Self, Error>;
    fn get_buffer_mut(&mut self) -> &mut Self::Buffer;
    fn clear_buffer(&mut self, color: Self::Color) -> Result<(), Error>;
    fn show_buffer(&mut self) -> Result<(), Error>;
    fn clear_display(&mut self) -> Result<(), Error>;
    fn sleep_device(&mut self) -> Result<(), Error>;
}

fn main() -> Result<(), std::io::Error> {
    let mut backend = Backend::open()?;

    {
        let buffer = backend.get_buffer_mut();

        buffer.draw(
            Font6x8::render_str("Rotate 270!")
                .stroke(Some(Backend::BLACK))
                .fill(Some(Backend::WHITE))
                .translate(Coord::new(5, 50))
                .into_iter(),
        );
    }

    backend.show_buffer()?;

    println!("Immediate custom test!");
    backend.clear_buffer(Backend::WHITE)?;

    {
        let buffer = backend.get_buffer_mut();

        // draw a analog clock
        buffer.draw(
            Circle::new(Coord::new(64, 64), 64)
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );
        buffer.draw(
            Line::new(Coord::new(64, 64), Coord::new(0, 64))
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );
        buffer.draw(
            Line::new(Coord::new(64, 64), Coord::new(80, 80))
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );

        // draw white on black background
        buffer.draw(
            Font6x8::render_str("It's working-WoB!")
                // Using Style here
                .style(Style {
                    fill_color: Some(Backend::BLACK),
                    stroke_color: Some(Backend::WHITE),
                    stroke_width: 0u8, // Has no effect on fonts
                })
                .translate(Coord::new(175, 250))
                .into_iter(),
        );

        // use bigger/different font
        buffer.draw(
            Font12x16::render_str("Hello World from Rust!")
                // Using Style here
                .style(Style {
                    fill_color: Some(Backend::WHITE),
                    stroke_color: Some(Backend::BLACK),
                    stroke_width: 0u8, // Has no effect on fonts
                })
                .translate(Coord::new(50, 200))
                .into_iter(),
        );
    }

    backend.show_buffer()?;

    println!("Finished tests - going to sleep");
    backend.sleep_device()?;

    Ok(())
}
