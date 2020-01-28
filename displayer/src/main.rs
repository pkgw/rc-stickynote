//! The program that renders information to the e-Print Display. (Or a
//! simulated version thereof.)

use embedded_graphics::{
    coord::Coord,
    fonts::{Font12x16, Font6x8},
    prelude::*,
    primitives::{Circle, Line},
    Drawing,
};
use std::{io::Error, thread, time::Duration};
use structopt::StructOpt;

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

// clear-and-sleep subcommand

#[derive(Debug, StructOpt)]
pub struct ClearAndSleepCommand {}

impl ClearAndSleepCommand {
    fn cli(self) -> Result<(), Error> {
        let mut backend = Backend::open()?;
        backend.clear_display()?;
        backend.sleep_device()?;
        Ok(())
    }
}

// show-ips subcommand

#[derive(Debug, StructOpt)]
pub struct ShowIpsCommand {}

impl ShowIpsCommand {
    fn cli(self) -> Result<(), Error> {
        let mut backend = Backend::open()?;

        {
            let buffer = backend.get_buffer_mut();
            let mut got_any = false;

            // If this program is set up to run on boot, the WiFi might not be
            // fully set up by the time we get here. So, retry several times
            // if we don't find any interesting IP addresses.

            for _ in 0..10 {
                // Note that we don't need to clear the buffer here, since the only
                // time we loop is when the buffer's contents are trivial.

                let mut y = 50;

                buffer.draw(
                    Font6x8::render_str("IP addresses:")
                        .style(Style {
                            fill_color: Some(Backend::WHITE),
                            stroke_color: Some(Backend::BLACK),
                            stroke_width: 0u8, // Has no effect on fonts
                        })
                        .translate(Coord::new(50, y))
                        .into_iter(),
                );

                y += 20;

                for iface in &get_if_addrs::get_if_addrs()? {
                    if !iface.is_loopback() {
                        if let get_if_addrs::IfAddr::V4(ref addr) = iface.addr {
                            let text = format!("{}   {}", iface.name, addr.ip);

                            buffer.draw(
                                Font6x8::render_str(&text)
                                    .style(Style {
                                        fill_color: Some(Backend::WHITE),
                                        stroke_color: Some(Backend::BLACK),
                                        stroke_width: 0u8, // Has no effect on fonts
                                    })
                                    .translate(Coord::new(50, y))
                                    .into_iter(),
                            );

                            y += 10;
                            got_any = true;
                        }
                    }
                }

                if got_any {
                    break;
                }

                thread::sleep(Duration::from_millis(10_000));
            }

            if !got_any {
                return Err(Error::new(
                    std::io::ErrorKind::Other,
                    "never got any useful IP addresses",
                ));
            }
        }

        backend.show_buffer()?;
        Ok(())
    }
}

// CLI root interface

#[derive(Debug, StructOpt)]
#[structopt(name = "displayer", about = "e-Ink Displayer tools")]
enum RootCli {
    #[structopt(name = "clear-and-sleep")]
    /// Clear the display and sleep the device
    ClearAndSleep(ClearAndSleepCommand),

    #[structopt(name = "show-ips")]
    /// Show IP addresses on the display
    ShowIps(ShowIpsCommand),
}

impl RootCli {
    fn cli(self) -> Result<(), Error> {
        match self {
            RootCli::ClearAndSleep(opts) => opts.cli(),
            RootCli::ShowIps(opts) => opts.cli(),
        }
    }
}

fn main() -> Result<(), Error> {
    RootCli::from_args().cli()
}
