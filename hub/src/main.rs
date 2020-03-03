//! The hub that brokers events between clients and the displayer panel.

#![recursion_limit = "256"]

use futures::{prelude::*, select};
use hmac::{Hmac, Mac};
use hyper::{
    header,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server,
};
use rc_stickynote_protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use std::{
    fs::File,
    io::{stdin, stdout, Error, Read, Write},
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

// Configuration and state for the hub program

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
    env_name: String,
    webhook_url: String,
    consumer_api_key: String,
    consumer_api_secret_key: String,
    access_token: String,
    access_token_secret: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ServerState {
    twitter: ServerTwitterState,
}

impl Default for ServerState {
    fn default() -> Self {
        ServerState {
            twitter: ServerTwitterState::default(),
        }
    }
}

impl ServerState {
    fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let mut f = File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(toml::from_slice(&buf[..])?)
    }

    fn try_load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        use std::io::ErrorKind::NotFound;

        match File::open(path) {
            Ok(mut f) => {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                Ok(toml::from_slice(&buf[..])?)
            }

            Err(e) => {
                if e.kind() == NotFound {
                    Ok(ServerState::default())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), GenericError> {
        let mut f = File::create(path)?;
        let data = toml::to_string(self)?;
        f.write_all(data.as_bytes())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ServerTwitterState {
    access_token: String,
    access_token_secret: String,
}

impl Default for ServerTwitterState {
    fn default() -> Self {
        ServerTwitterState {
            access_token: "invalid".to_owned(),
            access_token_secret: "invalid".to_owned(),
        }
    }
}

impl ServerTwitterState {
    fn get_token(&self, config: &ServerConfiguration) -> egg_mode::Token {
        let con_token = egg_mode::KeyPair::new(
            config.twitter.consumer_api_key.clone(),
            config.twitter.consumer_api_secret_key.clone(),
        );

        let access_token =
            egg_mode::KeyPair::new(self.access_token.clone(), self.access_token_secret.clone());

        egg_mode::Token::Access {
            consumer: con_token,
            access: access_token,
        }
    }
}

// "serve" subcommand

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
        let http_config = config.clone();
        let http_service = make_service_fn(move |_| {
            let http_config = http_config.clone();

            async {
                Ok::<_, GenericError>(service_fn(move |req| {
                    handle_http_request(req, http_config.clone())
                }))
            }
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

async fn handle_http_request(
    req: Request<Body>,
    config: ServerConfiguration,
) -> Result<Response<Body>, GenericError> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/webhooks/twitter") => handle_twitter_webhook_get(req, &config).await,

        _ => Ok(Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body((&b"not found"[..]).into())
            .unwrap()),
    }
}

/// This function must perform Twitter's "challenge-response check" (CRC, but
/// not the one you're used to.
async fn handle_twitter_webhook_get(
    req: Request<Body>,
    config: &ServerConfiguration,
) -> Result<Response<Body>, GenericError> {
    println!("handling Twitter challenge-response check");

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

    // Do the computation.

    let key = config.twitter.consumer_api_secret_key.as_bytes();
    let mut mac = Hmac::<Sha256>::new_varkey(key).expect("uhoh");
    mac.input(crc_token.as_bytes());
    let result = mac.result();
    let enc = base64::encode(&result.code());

    // Respond.

    let resp_val = json!({ "response_token": format!("sha256={}", enc) });
    let resp_json = serde_json::to_string(&resp_val)?;
    let response = Response::builder()
        .status(hyper::StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(resp_json))?;
    Ok(response)
}

// "twitter-login" subcommand

#[derive(Debug, StructOpt)]
pub struct TwitterLoginCommand {
    #[structopt(help = "The path to the server configuration file")]
    config_path: PathBuf,

    #[structopt(help = "The path to the server state file (need not exist)")]
    state_path: PathBuf,
}

impl TwitterLoginCommand {
    async fn cli(self) -> Result<(), GenericError> {
        let config = ServerConfiguration::load(&self.config_path)?;
        let mut state = ServerState::try_load(&self.state_path)?;

        println!("Beginning authentication flow ...");
        let con_token = egg_mode::KeyPair::new(
            config.twitter.consumer_api_key,
            config.twitter.consumer_api_secret_key,
        );
        let req_token = egg_mode::request_token(&con_token, "oob").await?;
        let auth_url = egg_mode::authorize_url(&req_token);
        print!(
            "Visit the following URL and obtain a verification PIN:\n\n\
             {}\n\n\
             Then enter the PIN here: ",
            auth_url
        );
        stdout().flush()?;

        let mut pin: String = String::new();
        stdin().read_line(&mut pin)?;

        let (token, user_id, screen_name) =
            egg_mode::access_token(con_token, &req_token, pin).await?;
        println!("Authenticated as @{} (user ID {})", screen_name, user_id);

        match token {
            egg_mode::Token::Access {
                access: ref access_token,
                ..
            } => {
                state.twitter.access_token = access_token.key.to_string();
                state.twitter.access_token_secret = access_token.secret.to_string();
            }

            _ => panic!("expected Access-type token"),
        }

        state.save(&self.state_path)?;

        Ok(())
    }
}

// "twitter-register-webhook" subcommand

#[derive(Debug, StructOpt)]
pub struct TwitterRegisterWebhookCommand {
    #[structopt(help = "The path to the server configuration file")]
    config_path: PathBuf,

    #[structopt(help = "The path to the server state file")]
    state_path: PathBuf,
}

impl TwitterRegisterWebhookCommand {
    async fn cli(self) -> Result<(), GenericError> {
        let config = ServerConfiguration::load(&self.config_path)?;
        let state = ServerState::load(&self.state_path)?;
        let token = state.twitter.get_token(&config);
        let hookspec = egg_mode::activity::WebhookSpec::new(&config.twitter.webhook_url);
        let result = hookspec.register(&config.twitter.env_name, &token).await?;
        println!("registered webhook: {:?}", result);
        Ok(())
    }
}

// "twitter-unregister-webhook" subcommand

#[derive(Debug, StructOpt)]
pub struct TwitterUnregisterWebhookCommand {
    #[structopt(help = "The path to the server configuration file")]
    config_path: PathBuf,

    #[structopt(help = "The path to the server state file")]
    state_path: PathBuf,

    /// TODO: if we really want this workflow to be reliable, we should save
    /// this ID in the state file.
    #[structopt(long = "id", help = "The ID of the webhook")]
    hook_id: String,
}

impl TwitterUnregisterWebhookCommand {
    async fn cli(self) -> Result<(), GenericError> {
        let config = ServerConfiguration::load(&self.config_path)?;
        let state = ServerState::load(&self.state_path)?;
        let token = state.twitter.get_token(&config);
        egg_mode::activity::delete_webhook(&config.twitter.env_name, &self.hook_id, &token).await?;
        println!("deregistered webhook");
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

    #[structopt(name = "twitter-register-webhook")]
    /// Register the activity webhook with Twitter
    TwitterRegisterWebhook(TwitterRegisterWebhookCommand),

    #[structopt(name = "twitter-unregister-webhook")]
    /// Un-register the activity webhook with Twitter
    TwitterUnregisterWebhook(TwitterUnregisterWebhookCommand),
}

impl RootCli {
    async fn cli(self) -> Result<(), GenericError> {
        match self {
            RootCli::Serve(opts) => opts.cli().await,
            RootCli::TwitterLogin(opts) => opts.cli().await,
            RootCli::TwitterRegisterWebhook(opts) => opts.cli().await,
            RootCli::TwitterUnregisterWebhook(opts) => opts.cli().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), GenericError> {
    RootCli::from_args().cli().await
}
