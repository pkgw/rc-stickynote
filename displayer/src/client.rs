//! The long-running panel driving client.

use embedded_graphics::{
    coord::Coord,
    fonts::{Font, Font6x8},
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

        loop {
            select! {
                msg = jsonread.try_next().fuse() => {
                    { let _type_inference: &Result<Option<protocol::DisplayMessage>, _> = &msg; }

                    match msg {
                        Ok(Some(m)) => {
                            println!("msg: {:?}", m);
                            if sender.send(m).is_err() {
                                return Err(Error::new(std::io::ErrorKind::Other, "display thread died?!"));
                            }
                        },

                        Ok(None) => break,

                        Err(err) => return Err(err),
                    }
                }

                _ = interval.tick().fuse() => {
                    println!("tick");
                }
            }
        }

        Ok(())
    })
}

fn renderer_thread(receiver: Receiver<protocol::DisplayMessage>) -> Result<(), std::io::Error> {
    // Note that Backend is not Send, so we have to open it up in this thread.
    let mut backend = Backend::open()?;

    loop {
        let msg = match receiver.recv() {
            Ok(m) => m,
            Err(_) => break,
        };

        {
            let buffer = backend.get_buffer_mut();

            buffer.draw(
                Font6x8::render_str(&msg.message)
                    .style(Style {
                        fill_color: Some(Backend::WHITE),
                        stroke_color: Some(Backend::BLACK),
                        stroke_width: 0u8, // Has no effect on fonts
                    })
                    .translate(Coord::new(50, 50))
                    .into_iter(),
            );
        }

        backend.show_buffer()?;
    }

    Ok(())
}
