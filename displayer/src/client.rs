//! The long-running panel driving client.

use embedded_graphics::{
    coord::Coord,
    fonts::{Font, Font6x8},
    pixelcolor::PixelColor,
    style::{Style, WithStyle},
    transform::Transform,
    Drawing,
};
use futures::{prelude::*, select};
use protocol::HelloMessage;
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

#[derive(Deserialize)]
struct ClientConfiguration {
    hub_host: String,
    hub_port: u16,
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
    thread::spawn(move || renderer_thread(receiver));

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

fn renderer_thread(receiver: Receiver<DisplayData>) -> Result<(), std::io::Error> {
    // Note that Backend is not Send, so we have to open it up in this thread.
    let mut backend = Backend::open()?;

    loop {
        // Zip through the channel until we find the very latest message.
        // We might be able to do this with a mutex on a scalar value, but
        // this way our thread can be woken up immediately when a new
        // message arrives.

        let mut display_data = match receiver.recv() {
            Ok(dd) => dd,
            Err(_) => break,
        };

        loop {
            match receiver.try_recv() {
                Ok(dd) => display_data = dd,

                // This error might be that the queue is empty, or that the
                // sender has disconnectd. If the latter, the error will come
                // up again in the next iteration, so we can actually handle
                // these two possibilities in the same way.
                Err(_) => break,
            };
        }

        // Draw. Keep in mind that on the actual device, this takes more than
        // 10 seconds!

        display_data.draw(backend.get_buffer_mut(), Backend::WHITE, Backend::BLACK);
        backend.show_buffer()?;
    }

    Ok(())
}


#[derive(Clone, Debug)]
struct DisplayData {
    scientist_is: String,
    ip_addr: String,
}

impl DisplayData {
    fn new() -> Result<Self, std::io::Error> {
        let mut dd = DisplayData {
            scientist_is: "???".to_owned(),
            ip_addr: "".to_owned(),
        };
        dd.update_local()?;
        Ok(dd)
    }

    fn update_local(&mut self) -> Result<(), std::io::Error> {
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

    fn draw<C: PixelColor, T: Drawing<C>>(&self, buffer: &mut T, white: C, black: C) {
        buffer.draw(
            Font6x8::render_str(&self.scientist_is)
                .style(Style {
                    fill_color: Some(white),
                    stroke_color: Some(black),
                    stroke_width: 0u8, // Has no effect on fonts
                })
                .translate(Coord::new(50, 50))
                .into_iter(),
        );

        buffer.draw(
            Font6x8::render_str(&self.ip_addr)
                .style(Style {
                    fill_color: Some(white),
                    stroke_color: Some(black),
                    stroke_width: 0u8, // Has no effect on fonts
                })
                .translate(Coord::new(50, 100))
                .into_iter(),
        );
    }
}
