//! The program that renders information to the e-Print Display. (Or a
//! simulated version thereof.)

use embedded_graphics::{coord::Coord, fonts::Font6x8, prelude::*, Drawing};
use rusttype::FontCollection;
use std::{
    fs::File,
    io::{Error, Read},
    path::PathBuf,
    thread,
    time::Duration,
};
use structopt::StructOpt;

#[cfg(feature = "waveshare")]
mod epd7in5;
#[cfg(feature = "waveshare")]
use epd7in5::EPD7in5Backend as Backend;

#[cfg(feature = "simulator")]
mod simulator;
#[cfg(feature = "simulator")]
use simulator::SimulatorBackend as Backend;

mod client;
mod text;
use text::DrawFontExt;

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

// client subcommand

#[derive(Debug, StructOpt)]
pub struct ClientCommand {}

impl ClientCommand {
    fn cli(self) -> Result<(), Error> {
        client::main_cli(self)
    }
}

// demo-font subcommand

#[derive(Debug, StructOpt)]
pub struct DemoFontCommand {
    #[structopt(help = "The path to a TTF or OTF font file.")]
    font_path: PathBuf,
}

impl DemoFontCommand {
    fn cli(self) -> Result<(), Error> {
        let mut file = File::open(&self.font_path)?;
        let mut font_data = Vec::new();
        file.read_to_end(&mut font_data)?;

        let collection = FontCollection::from_bytes(font_data)?;
        let font = collection.into_font()?; // only succeeds if collection consists of one font

        let mut backend = Backend::open()?;

        {
            let buffer = backend.get_buffer_mut();

            buffer.draw(
                font.rasterize("The quick brown fox jumps over the lazy dog.", 10.0)
                    .draw_at(10, 10, Backend::BLACK, Backend::WHITE),
            );

            buffer.draw(
                font.rasterize("The quick brown fox jumps over the lazy dog.", 14.0)
                    .draw_at(10, 30, Backend::BLACK, Backend::WHITE),
            );

            buffer.draw(font.rasterize("The quick brown fox", 20.0).draw_at(
                10,
                58,
                Backend::BLACK,
                Backend::WHITE,
            ));
            buffer.draw(font.rasterize("jumps over the lazy dog.", 20.0).draw_at(
                10,
                80,
                Backend::BLACK,
                Backend::WHITE,
            ));

            buffer.draw(font.rasterize("The quick brown fox", 32.0).draw_at(
                10,
                110,
                Backend::BLACK,
                Backend::WHITE,
            ));
            buffer.draw(font.rasterize("jumps over the lazy dog.", 32.0).draw_at(
                10,
                138,
                Backend::BLACK,
                Backend::WHITE,
            ));

            buffer.draw(font.rasterize("The quick brown", 48.0).draw_at(
                10,
                184,
                Backend::BLACK,
                Backend::WHITE,
            ));
            buffer.draw(font.rasterize("fox jumps over", 48.0).draw_at(
                10,
                230,
                Backend::BLACK,
                Backend::WHITE,
            ));
            buffer.draw(font.rasterize("the lazy dog.", 48.0).draw_at(
                10,
                276,
                Backend::BLACK,
                Backend::WHITE,
            ));
        }

        backend.show_buffer()?;
        Ok(())
    }
}

// set-status subcommand

#[derive(Debug, StructOpt)]
pub struct SetStatusCommand {
    status: String,
}

impl SetStatusCommand {
    fn cli(self) -> Result<(), Error> {
        client::set_status_cli(self)
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

    #[structopt(name = "client")]
    /// Launch a client that connects to a hub and drives the display.
    Client(ClientCommand),

    #[structopt(name = "demo-font")]
    /// Render a TrueType font at various sizes.
    DemoFont(DemoFontCommand),

    #[structopt(name = "set-status")]
    /// Set the "scientist is:" satus on the display
    SetStatus(SetStatusCommand),

    #[structopt(name = "show-ips")]
    /// Show IP addresses on the display
    ShowIps(ShowIpsCommand),
}

impl RootCli {
    fn cli(self) -> Result<(), Error> {
        match self {
            RootCli::ClearAndSleep(opts) => opts.cli(),
            RootCli::Client(opts) => opts.cli(),
            RootCli::DemoFont(opts) => opts.cli(),
            RootCli::SetStatus(opts) => opts.cli(),
            RootCli::ShowIps(opts) => opts.cli(),
        }
    }
}

fn main() -> Result<(), Error> {
    RootCli::from_args().cli()
}
