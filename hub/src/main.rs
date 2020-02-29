//! The hub that brokers events between clients and the displayer panel.

#![recursion_limit = "256"]

use futures::{prelude::*, select};
use hyper::{
    header,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server,
};
use rc_stickynote_protocol::*;
use serde::Deserialize;
use std::{
    fs::File,
    io::{Error, Read},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::{channel, Sender},
    time::{self, Duration},
};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

// "serve" subcommand

type GenericError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug, Deserialize)]
struct ServerConfiguration {
    stickyproto_port: u16,
    http_port: u16,
    twitter: ServerTwitterConfiguration,
}

impl ServerConfiguration {
    fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let mut f = File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(toml::from_slice(&buf[..])?)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ServerTwitterConfiguration {
    consumer_api_key: String,
    consumer_api_secret_key: String,
    access_token: String,
    access_token_secret: String,
}

#[derive(Debug, StructOpt)]
pub struct ServeCommand {
    #[structopt(help = "The path to the server configuration file")]
    config_path: PathBuf,
}

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
    async fn cli(self) -> Result<(), GenericError> {
        let config = ServerConfiguration::load(&self.config_path)?;

        let (send_updates, mut receive_updates) = channel(4);
        let mut display_state = DisplayMessage::default();

        // Set up the stickynote protocol server

        let sp_host = Ipv4Addr::new(127, 0, 0, 1);
        let mut sp_listener = TcpListener::bind((sp_host, config.stickyproto_port))
            .await
            .unwrap();
        let mut sp_incoming = sp_listener.incoming();
        println!(
            "Stickynote protocol server running on {}:{}",
            sp_host, config.stickyproto_port
        );

        // Set up the HTTP server

        let http_host = sp_host;
        let http_service = make_service_fn(move |_| async {
            Ok::<_, GenericError>(service_fn(move |req| handle_http_request(req)))
        });
        let http_server =
            Server::bind(&SocketAddr::from((http_host, config.http_port))).serve(http_service);
        println!("HTTP server running on {}:{}", http_host, config.http_port);

        tokio::spawn(async move { http_server.await });

        // Stickynote event loop

        loop {
            select! {
                maybe_socket = sp_incoming.next().fuse() => {
                    match maybe_socket {
                        Some(Ok(sock)) => {
                            match handle_new_stickyproto_connection(sock, display_state.clone(), send_updates.clone()) {
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

fn handle_new_stickyproto_connection(
    mut socket: TcpStream,
    mut display_state: DisplayMessage,
    send_updates: Sender<DisplayStateMutation>,
) -> Result<(), Error> {
    println!(
        "Accepted stickyproto connection from {:?}",
        socket.peer_addr()
    );

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

async fn handle_http_request(req: Request<Body>) -> Result<Response<Body>, GenericError> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/webhooks/twitter") => handle_twitter_webhook_get(req).await,

        _ => Ok(Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body((&b"not found"[..]).into())
            .unwrap()),
    }
}

/// This function must perform Twitter's "challenge-response check" (CRC, but
/// not the one you're used to.
async fn handle_twitter_webhook_get(req: Request<Body>) -> Result<Response<Body>, GenericError> {
    // Get the crc_token argument.

    let mut crc_token = None;

    if let Some(qstring) = req.uri().query() {
        for (name, value) in url::form_urlencoded::parse(qstring.as_bytes()) {
            if name == "crc_token" {
                crc_token = Some(value);
            }
        }
    }

    let crc_token = match crc_token {
        Some(t) => t,

        None => {
            return Ok(Response::builder()
                .status(hyper::StatusCode::BAD_REQUEST)
                .body((&b"expected crc_token"[..]).into())
                .unwrap());
        }
    };

    // Temporary: demo code that hopefully does the Twitter challenge-response
    // check correctly.

    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    const CONSUMER_SECRET: &[u8] = b"SECRET";

    let mut mac = Hmac::<Sha256>::new_varkey(CONSUMER_SECRET).expect("uhoh");
    mac.input(crc_token.as_bytes());
    let result = mac.result();
    let enc = base64::encode(&result.code());

    println!("B64: {}", enc);

    // end temp

    let response = Response::builder()
        .status(hyper::StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok": true}"#))?;
    Ok(response)
}

// "twitter-login" subcommand

#[derive(Debug, StructOpt)]
pub struct TwitterLoginCommand {}

impl TwitterLoginCommand {
    async fn cli(self) -> Result<(), GenericError> {
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
    async fn cli(self) -> Result<(), GenericError> {
        match self {
            RootCli::Serve(opts) => opts.cli().await,
            RootCli::TwitterLogin(opts) => opts.cli().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), GenericError> {
    RootCli::from_args().cli().await
}
