//! The hub that brokers events between clients and the displayer panel.

use futures::prelude::*;
use rc_stickynote_protocol::*;
use std::io::Error;
use structopt::StructOpt;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{self, Duration},
};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

// "serve" subcommand

#[derive(Debug, StructOpt)]
pub struct ServeCommand {}

impl ServeCommand {
    async fn cli(self) -> Result<(), Error> {
        let addr = "127.0.0.1:20200";
        let mut listener = TcpListener::bind(addr).await.unwrap();
        let mut incoming = listener.incoming();
        println!("Server running on {}", addr);

        while let Some(socket_res) = incoming.next().await {
            match socket_res {
                Ok(socket) => match handle_new_connection(socket) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("error while setting up new connection: {:?}", e);
                    }
                },

                Err(err) => {
                    // Handle error by printing to STDOUT.
                    println!("accept error = {:?}", err);
                }
            }
        }

        Ok(())
    }
}

fn handle_new_connection(mut socket: TcpStream) -> Result<(), Error> {
    println!("Accepted connection from {:?}", socket.peer_addr());

    tokio::spawn(async move {
        let (read, write) = socket.split();
        let ldread = FramedRead::new(read, LengthDelimitedCodec::new());
        let mut jsonread = SymmetricallyFramed::new(ldread, SymmetricalJson::default());
        let ldwrite = FramedWrite::new(write, LengthDelimitedCodec::new());
        let mut jsonwrite = SymmetricallyFramed::new(ldwrite, SymmetricalJson::default());
        let hello: Option<Result<ClientHelloMessage, Error>> = jsonread.next().await;

        let hello = match hello {
            Some(Ok(h)) => h,
            Some(err) => return err,
            None => {
                return Err(Error::new(
                    std::io::ErrorKind::Other,
                    "connection dropped before hello?",
                ));
            }
        };

        match hello {
            ClientHelloMessage::PersonIsUpdate(msg) => {}

            ClientHelloMessage::Display(_) => {}
        };

        jsonwrite
            .send(DisplayMessage {
                person_is: "hello".to_owned(),
            })
            .await?;

        let mut interval = time::interval(Duration::from_millis(60_000));
        let mut tick = 0usize;

        loop {
            interval.tick().await;

            // temporary demo hack
            let person_is = if tick % 2 == 0 {
                "in"
            } else {
                "getting coffee"
            }
            .to_owned();

            jsonwrite.send(DisplayMessage { person_is }).await?;
            tick += 1;
        }
    });

    Ok(())
}

// "twitter-login" subcommand

#[derive(Debug, StructOpt)]
pub struct TwitterLoginCommand {}

impl TwitterLoginCommand {
    async fn cli(self) -> Result<(), Error> {
        Ok(())
    }
}

// CLI root interface

#[derive(Debug, StructOpt)]
#[structopt(name = "hub", about = "RC Stickynote dispatch hub")]
enum RootCli {
    #[structopt(name = "serve")]
    /// Launch the dispatch hub server.
    Serve(ServeCommand),

    #[structopt(name = "twitter-login")]
    /// Login to the connected Twitter account
    TwitterLogin(TwitterLoginCommand),
}

impl RootCli {
    async fn cli(self) -> Result<(), Error> {
        match self {
            RootCli::Serve(opts) => opts.cli().await,
            RootCli::TwitterLogin(opts) => opts.cli().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    RootCli::from_args().cli().await
}
