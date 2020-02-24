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
    SetPersonIs(PersonIsUpdateHelloMessage),
}

impl DisplayStateMutation {
    /// Apply the mutation defined by this value to the specified state
    /// object, consuming this value in the process.
    pub fn consume_into(self, state: &mut DisplayMessage) {
        match self {
            DisplayStateMutation::SetPersonIs(msg) => {
                state.person_is = msg.person_is;
                state.person_is_timestamp = msg.timestamp;
            }
        }
    }
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
                            match handle_new_connection(sock, display_state.clone(), send_updates.clone()) {
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
                        Some(Ok(mutation)) => mutation.consume_into(&mut display_state),

                        Some(Err(err)) => {
                            println!("receive_updates error = {}", err);
                        },

                        None => {
                            println!("receive_updates ran out??");
                        },
                    }
                },
            }
        }
    }
}

fn handle_new_connection(
    mut socket: TcpStream,
    mut display_state: DisplayMessage,
    send_updates: Sender<DisplayStateMutation>,
) -> Result<(), Error> {
    println!("Accepted connection from {:?}", socket.peer_addr());

    tokio::spawn(async move {
        let (read, write) = socket.split();
        let ldread = FramedRead::new(read, LengthDelimitedCodec::new());
        let mut jsonread = SymmetricallyFramed::new(ldread, SymmetricalJson::default());

        // Receive the initial "hello" message from the client.

        let hello = match jsonread.next().await {
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
                if !is_person_is_valid(&msg.person_is) {
                    // We could attempt to truncate it or something, but the
                    // system is tightly-coupled enough that I don't see the
                    // value in implementing that.
                    return Err(Error::new(
                        std::io::ErrorKind::Other,
                        "PersonIsUpdate message didn't validate; ignoring",
                    ));
                }

                // Just accept the update and we're done.
                return match send_updates.send(DisplayStateMutation::SetPersonIs(msg)) {
                    Ok(_) => Ok(()),
                    Err(_) => Err(Error::new(
                        std::io::ErrorKind::Other,
                        "no receivers for thread update?",
                    )),
                };
            }

            ClientHelloMessage::Display(_) => {}
        };

        // If we're still here, the client is a displayer and we should keep
        // it updated.

        let ldwrite = FramedWrite::new(write, LengthDelimitedCodec::new());
        let mut jsonwrite = SymmetricallyFramed::new(ldwrite, SymmetricalJson::default());
        let mut receive_updates = send_updates.subscribe();

        // We'll make sure to send the client an update at least this often. The
        // interval will fire immediately, which means that the client will get an
        // update right off the bat, as desired.
        let mut interval = time::interval(Duration::from_millis(1200_000));

        loop {
            select! {
                _ = interval.tick().fuse() => {},

                maybe_update = receive_updates.next().fuse() => {
                    match maybe_update {
                        Some(Ok(mutation)) => mutation.consume_into(&mut display_state),

                        Some(Err(err)) => {
                            println!("client receive_updates error = {}", err);
                        },

                        None => {
                            println!("client receive_updates ran out??");
                        },
                    }
                },
            }

            jsonwrite.send(display_state.clone()).await?;
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
