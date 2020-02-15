//! The hub that brokers events between clients and the displayer panel.

#![recursion_limit = "256"]

use futures::{prelude::*, select};
use rc_stickynote_protocol::*;
use std::io::Error;
use structopt::StructOpt;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::{channel, Sender},
    time::{self, Duration},
};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

// "serve" subcommand

#[derive(Debug, StructOpt)]
pub struct ServeCommand {}

#[derive(Clone, Debug)]
enum DisplayStateMutation {
    SetPersonIs(String),
}

impl ServeCommand {
    async fn cli(self) -> Result<(), Error> {
        let addr = "127.0.0.1:20200";
        let mut listener = TcpListener::bind(addr).await.unwrap();
        let mut incoming = listener.incoming();
        println!("Server running on {}", addr);

        let (send_updates, mut receive_updates) = channel(4);
        let mut display_state = DisplayMessage::default();

        loop {
            select! {
                maybe_socket = incoming.next().fuse() => {
                    match maybe_socket {
                        Some(Ok(sock)) => {
                            match handle_new_connection(sock, send_updates.clone()) {
                                Ok(_) => {}
                                Err(e) => {
                                    println!("error while setting up new connection: {:?}", e);
                                }
                            }
                        },

                        Some(Err(err)) => {
                            // Handle error by printing to STDOUT.
                            println!("accept error = {:?}", err);
                        },

                        None => {
                            println!("socket ran out??");
                        },
                    }
                },

                maybe_update = receive_updates.next().fuse() => {
                    match maybe_update {
                        Some(Ok(DisplayStateMutation::SetPersonIs(msg))) => { display_state.person_is = msg; },

                        Some(Err(err)) => {
                            println!("receive_updates error = {}", err);
                        },

                        None => {
                            println!("receive_update ran out??");
                        },
                    }
                },
            }
        }
    }
}

fn handle_new_connection(
    mut socket: TcpStream,
    send_updates: Sender<DisplayStateMutation>,
) -> Result<(), Error> {
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
            Some(Err(err)) => {
                return Err(Error::new(std::io::ErrorKind::Other, err.to_string()));
            }
            None => {
                return Err(Error::new(
                    std::io::ErrorKind::Other,
                    "connection dropped before hello?",
                ));
            }
        };

        match hello {
            ClientHelloMessage::PersonIsUpdate(msg) => {
                // Just accept the update and we're done.
                return match send_updates.send(DisplayStateMutation::SetPersonIs(msg.person_is)) {
                    Ok(_) => Ok(()),
                    Err(_) => Err(Error::new(
                        std::io::ErrorKind::Other,
                        "no receivers for thread update?",
                    )),
                };
            }

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
