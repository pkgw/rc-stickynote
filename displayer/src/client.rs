//! The long-running panel driving client.

use chrono::prelude::*;
use embedded_graphics::{
    coord::Coord,
    fonts::{Font, Font6x8},
    primitives::{Line, Rectangle},
    style::{Style, WithStyle},
    transform::Transform,
    Drawing,
};
use futures::{prelude::*, select};
use rc_stickynote_protocol::{
    is_person_is_valid, ClientHelloMessage, DisplayHelloMessage, DisplayMessage,
    PersonIsUpdateHelloMessage,
};
use rusttype::FontCollection;
use serde::Deserialize;
use std::{
    fs::File,
    io::{Error, Read},
    sync::mpsc::{channel, Receiver},
    thread,
};
use tokio::{
    net::TcpStream,
    runtime::Runtime,
    time::{self, Duration},
};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use toml;

use super::{Backend, DisplayBackend};
use crate::text::DrawFontExt;

#[derive(Clone, Deserialize)]
struct ClientConfiguration {
    hub_host: String,
    hub_port: u16,
    sans_path: String,
    serif_path: String,
}

pub fn main_cli(opts: super::ClientCommand) -> Result<(), Error> {
    // Parse the configuration.

    let config: ClientConfiguration = {
        let mut f = File::open(&opts.config_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        toml::from_slice(&buf[..])?
    };

    let (sender, receiver) = channel();

    // The actual renderer operates in its own thread since the I/O can be slow
    // and we don't want to block the async runtime.
    let cloned_config = config.clone();
    thread::spawn(move || renderer_thread(cloned_config, receiver));

    let mut rt = Runtime::new()?;

    rt.block_on(async {
        let mut hub_connection =
            TcpStream::connect((config.hub_host.as_ref(), config.hub_port)).await?;
        let (hub_read, hub_write) = hub_connection.split();
        let ldread = FramedRead::new(hub_read, LengthDelimitedCodec::new());
        let mut jsonread = SymmetricallyFramed::new(ldread, SymmetricalJson::default());
        let ldwrite = FramedWrite::new(hub_write, LengthDelimitedCodec::new());
        let mut jsonwrite = SymmetricallyFramed::new(ldwrite, SymmetricalJson::default());

        // Say hello.
        jsonwrite
            .send(ClientHelloMessage::Display(DisplayHelloMessage {}))
            .await?;

        let mut interval = time::interval(Duration::from_millis(600_000));

        let mut display_data = DisplayData::new()?;

        loop {
            // `select` on various things that might motivate us to update the
            // display.

            select! {
                // New message from the hub.
                msg = jsonread.try_next().fuse() => {
                    { let _type_inference: &Result<Option<DisplayMessage>, _> = &msg; }

                    match msg {
                        Ok(Some(m)) => {
                            println!("msg: {:?}", m);
                            display_data.person_is = m.person_is;
                        },

                        Ok(None) => break,

                        Err(err) => return Err(err),
                    }
                }

                // Time has passed since the last interval tick.
                _ = interval.tick().fuse() => {
                    println!("local tick");
                    display_data.update_local()?;
                }
            }

            // Send the current state over to the display thread!
            if sender.send(display_data.clone()).is_err() {
                return Err(Error::new(
                    std::io::ErrorKind::Other,
                    "display thread died?!",
                ));
            }
        }

        Ok(())
    })
}

fn renderer_thread(config: ClientConfiguration, receiver: Receiver<DisplayData>) {
    if let Err(e) = renderer_thread_inner(config, receiver) {
        eprintln!("ERROR: rendererer thread exited with error: {}", e);
    }
}

fn renderer_thread_inner(
    config: ClientConfiguration,
    receiver: Receiver<DisplayData>,
) -> Result<(), std::io::Error> {
    // Note that Backend is not Send, so we have to open it up in this thread.
    let mut backend = Backend::open()?;

    let sans_font = {
        let mut file = File::open(&config.sans_path)?;
        let mut font_data = Vec::new();
        file.read_to_end(&mut font_data)?;
        let collection = FontCollection::from_bytes(font_data)?;
        collection.into_font()?
    };

    let serif_font = {
        let mut file = File::open(&config.serif_path)?;
        let mut font_data = Vec::new();
        file.read_to_end(&mut font_data)?;
        let collection = FontCollection::from_bytes(font_data)?;
        collection.into_font()?
    };

    loop {
        // Zip through the channel until we find the very latest message.
        // We might be able to do this with a mutex on a scalar value, but
        // this way our thread can be woken up immediately when a new
        // message arrives.

        let mut dd = match receiver.recv() {
            Ok(dd) => dd,
            Err(_) => break,
        };

        loop {
            match receiver.try_recv() {
                Ok(new_dd) => dd = new_dd,

                // This error might be that the queue is empty, or that the
                // sender has disconnectd. If the latter, the error will come
                // up again in the next iteration, so we can actually handle
                // these two possibilities in the same way.
                Err(_) => break,
            };
        }

        // Render into the buffer.

        {
            let buffer = backend.get_buffer_mut();

            fn draw6x8(buf: &mut <Backend as DisplayBackend>::Buffer, s: &str, x: i32, y: i32) {
                buf.draw(
                    Font6x8::render_str(s)
                        .style(Style {
                            fill_color: Some(Backend::WHITE),
                            stroke_color: Some(Backend::BLACK),
                            stroke_width: 0u8, // Has no effect on fonts
                        })
                        .translate(Coord::new(x, y))
                        .into_iter(),
                );
            }

            // The clock

            let now = dd.now.format("%I:%M %p").to_string();

            buffer.draw(sans_font.rasterize(&now, 56.0).draw_at(
                2,
                0,
                Backend::BLACK,
                Backend::WHITE,
            ));

            let x = 230;
            let y = 8;
            let delta = 10;

            draw6x8(buffer, "May be up to 15 minutes", x, y + 0 * delta);
            draw6x8(buffer, "out of date. If much more", x, y + 1 * delta);
            draw6x8(buffer, "than that, tell Peter his", x, y + 2 * delta);
            draw6x8(buffer, "sticky note is broken.", x, y + 3 * delta);

            // hline

            buffer.draw(
                Line::new(Coord::new(0, 52), Coord::new(383, 52)).style(Style {
                    fill_color: Some(Backend::BLACK),
                    stroke_color: Some(Backend::BLACK),
                    stroke_width: 1u8,
                }),
            );

            // "Scientist is ..."

            let x = 8;
            let y = 54;
            let delta = 54;

            buffer.draw(serif_font.rasterize("The Innovation", 64.0).draw_at(
                x,
                y,
                Backend::BLACK,
                Backend::WHITE,
            ));

            buffer.draw(serif_font.rasterize("Scientist is:", 64.0).draw_at(
                x + 2,
                y + delta,
                Backend::BLACK,
                Backend::WHITE,
            ));

            let y = y + 2 * delta + 12;
            let delta = delta;

            buffer.draw(
                Rectangle::new(Coord::new(0, y), Coord::new(383, y + delta))
                    .fill(Some(Backend::BLACK)),
            );

            let layout = sans_font.rasterize(&dd.person_is, 32.0);
            let x = if layout.width as i32 > 384 {
                0
            } else {
                (384 - layout.width as i32) / 2
            };
            let yofs = if layout.height as i32 > delta {
                0
            } else {
                (delta - layout.height as i32) / 2
            };

            buffer.draw(layout.draw_at(x, y + yofs, Backend::WHITE, Backend::BLACK));

            // IP address

            let x = 382 - 6 * (dd.ip_addr.len() as i32);
            draw6x8(buffer, &dd.ip_addr, x, 631);
        }

        // Push the buffer. Keep in mind that on the actual device, this takes
        // more than 10 seconds!

        backend.show_buffer()?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct DisplayData {
    pub now: DateTime<Local>,
    pub person_is: String,
    pub ip_addr: String,
}

impl DisplayData {
    fn new() -> Result<Self, std::io::Error> {
        let mut dd = DisplayData {
            now: Local::now(),
            person_is: "???".to_owned(),
            ip_addr: "".to_owned(),
        };
        dd.update_local()?;
        Ok(dd)
    }

    fn update_local(&mut self) -> Result<(), std::io::Error> {
        self.now = Local::now();

        self.ip_addr = "???.???.???.???".to_owned();

        for iface in &get_if_addrs::get_if_addrs()? {
            if !iface.is_loopback() {
                if let get_if_addrs::IfAddr::V4(ref addr) = iface.addr {
                    self.ip_addr = addr.ip.to_string();
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Send a status update to the hub. This uses the same infrastructure as the
/// main client but is way simpler.
pub fn set_status_cli(opts: super::SetStatusCommand) -> Result<(), Error> {
    if !is_person_is_valid(&opts.status) {
        return Err(Error::new(
            std::io::ErrorKind::Other,
            format!("status \"{}\" invalid -- likely too long", &opts.status),
        ));
    }

    let config: ClientConfiguration = {
        let mut f = File::open(&opts.config_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        toml::from_slice(&buf[..])?
    };

    let mut rt = Runtime::new()?;

    rt.block_on(async {
        let hub_connection =
            TcpStream::connect((config.hub_host.as_ref(), config.hub_port)).await?;
        let ldwrite = FramedWrite::new(hub_connection, LengthDelimitedCodec::new());
        let mut jsonwrite = SymmetricallyFramed::new(ldwrite, SymmetricalJson::default());

        jsonwrite
            .send(ClientHelloMessage::PersonIsUpdate(
                PersonIsUpdateHelloMessage {
                    person_is: opts.status,
                },
            ))
            .await?;

        Ok(())
    })
}
