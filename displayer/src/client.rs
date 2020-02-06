//! The long-running panel driving client.

use chrono::prelude::*;
use embedded_graphics::{
    coord::Coord,
    fonts::{Font, Font6x8},
    style::{Style, WithStyle},
    transform::Transform,
    Drawing,
};
use futures::{prelude::*, select};
use protocol::HelloMessage;
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

use crate::text::DrawFontExt;
use super::{Backend, DisplayBackend};

#[derive(Clone, Deserialize)]
struct ClientConfiguration {
    hub_host: String,
    hub_port: u16,
    sans_path: String,
}

pub fn cli(opts: super::ClientCommand) -> Result<(), Error> {
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
        jsonwrite.send(HelloMessage { a_number: 123 }).await?;

        let mut interval = time::interval(Duration::from_millis(15_000));

        let mut display_data = DisplayData::new()?;

        loop {
            // `select` on various things that might motivate us to update the
            // display.

            select! {
                // New message from the hub.
                msg = jsonread.try_next().fuse() => {
                    { let _type_inference: &Result<Option<protocol::DisplayMessage>, _> = &msg; }

                    match msg {
                        Ok(Some(m)) => {
                            println!("msg: {:?}", m);
                            display_data.scientist_is = m.message;
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
                return Err(Error::new(std::io::ErrorKind::Other, "display thread died?!"));
            }
        }

        Ok(())
    })
}

fn renderer_thread(config: ClientConfiguration, receiver: Receiver<DisplayData>) -> Result<(), std::io::Error> {
    // Note that Backend is not Send, so we have to open it up in this thread.
    let mut backend = Backend::open()?;

    let sans_font = {
        let mut file = File::open(&config.sans_path)?;
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

        // Draw. Keep in mind that on the actual device, this takes more than
        // 10 seconds!

        {
            let buffer = backend.get_buffer_mut();

            let now = dd.now.format("%I:%M %p").to_string();

            buffer.draw(sans_font.rasterize(&now, 48.0).draw_at(
                50, 50,
                Backend::BLACK, Backend::WHITE,
            ));

            buffer.draw(
                Font6x8::render_str(&dd.scientist_is)
                    .style(Style {
                        fill_color: Some(Backend::WHITE),
                        stroke_color: Some(Backend::BLACK),
                        stroke_width: 0u8, // Has no effect on fonts
                    })
                    .translate(Coord::new(50, 100))
                    .into_iter(),
            );

            buffer.draw(
                Font6x8::render_str(&dd.ip_addr)
                    .style(Style {
                        fill_color: Some(Backend::WHITE),
                        stroke_color: Some(Backend::BLACK),
                        stroke_width: 0u8, // Has no effect on fonts
                    })
                    .translate(Coord::new(50, 150))
                    .into_iter(),
            );
        }

        backend.show_buffer()?;
    }

    Ok(())
}


#[derive(Clone, Debug)]
struct DisplayData {
    pub now: DateTime<Local>,
    pub scientist_is: String,
    pub ip_addr: String,
}

impl DisplayData {
    fn new() -> Result<Self, std::io::Error> {
        let mut dd = DisplayData {
            now: Local::now(),
            scientist_is: "???".to_owned(),
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
