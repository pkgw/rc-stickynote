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
    type Display: Drawing<Self::Color>;

    const BLACK: Self::Color;
    const WHITE: Self::Color;

    fn open() -> Result<Self, Error>;
    fn get_display_mut(&mut self) -> &mut Self::Display;
    fn clear(&mut self, color: Self::Color) -> Result<(), Error>;
    fn show(&mut self) -> Result<(), Error>;
    fn sleep(&mut self) -> Result<(), Error>;
}

fn main() -> Result<(), std::io::Error> {
    let mut backend = Backend::open()?;

    {
        let display = backend.get_display_mut();

        display.draw(
            Font6x8::render_str("Rotate 270!")
                .stroke(Some(Backend::BLACK))
                .fill(Some(Backend::WHITE))
                .translate(Coord::new(5, 50))
                .into_iter(),
        );
    }

    backend.show()?;

    println!("Immediate custom test!");
    backend.clear(Backend::WHITE)?;

    {
        let display = backend.get_display_mut();

        // draw a analog clock
        display.draw(
            Circle::new(Coord::new(64, 64), 64)
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );
        display.draw(
            Line::new(Coord::new(64, 64), Coord::new(0, 64))
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );
        display.draw(
            Line::new(Coord::new(64, 64), Coord::new(80, 80))
                .stroke(Some(Backend::BLACK))
                .into_iter(),
        );

        // draw white on black background
        display.draw(
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
        display.draw(
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

    backend.show()?;

    println!("Finished tests - going to sleep");
    backend.sleep()?;

    Ok(())
}
