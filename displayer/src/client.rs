//! The long-running panel driving client.

use futures::{prelude::*, select};
use protocol::HelloMessage;
use serde::Deserialize;
use std::{
    fs::File,
    io::{Error, Read},
};
use tokio::{net::TcpStream, runtime::Runtime, time::{self, Duration}};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use toml;

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

    let mut rt = Runtime::new()?;

    rt.block_on(async {
        let mut hub_connection = TcpStream::connect((config.hub_host.as_ref(), config.hub_port)).await?;
        let (hub_read, hub_write) = hub_connection.split();
        let ldread = FramedRead::new(hub_read, LengthDelimitedCodec::new());
        let mut jsonread = SymmetricallyFramed::new(ldread, SymmetricalJson::default());
        let ldwrite = FramedWrite::new(hub_write, LengthDelimitedCodec::new());
        let mut jsonwrite = SymmetricallyFramed::new(ldwrite, SymmetricalJson::default());

        // Say hello.
        jsonwrite.send(HelloMessage { a_number: 123 }).await?;

        let mut interval = time::interval(Duration::from_millis(1_000));

        loop {
            select! {
                msg = jsonread.try_next().fuse() => {
                    { let _type_inference: &Result<Option<protocol::DisplayMessage>, _> = &msg; }

                    match msg {
                        Ok(Some(m)) => {
                            println!("msg: {:?}", m);
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
