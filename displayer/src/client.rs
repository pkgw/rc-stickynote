//! The long-running panel driving client.

use chrono::prelude::*;
use daemonize::Daemonize;
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
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Error, Read},
    net::TcpStream as StdTcpStream,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    thread,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    runtime::Runtime,
    time::{self, Duration},
};
use tokio_serde::{formats::Json, Framed as SerdeFramed};
use tokio_util::codec::{Framed as CodecFramed, LengthDelimitedCodec};

use super::{Backend, DisplayBackend};
use crate::text::DrawFontExt;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClientConfiguration {
    hub_host: String,
    hub_port: u16,
    ssh: Option<ClientSshConfiguration>,
    sans_path: String,
    serif_path: String,
}

impl Default for ClientConfiguration {
    fn default() -> Self {
        ClientConfiguration {
            hub_host: "edit-configuration.example.com".to_owned(),
            hub_port: 20200,
            ssh: None,
            sans_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_owned(),
            serif_path: "/usr/share/fonts/truetype/freefont/FreeSerif.ttf".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClientSshConfiguration {
    private_key_path: String,
    ssh_port: u16,
    user: String,
}

/// Lame analogue of `try!` for SSH results, adapting their error type from
/// async_ssh2's to std::io::Error.
macro_rules! tryssh {
    ($e:expr) => {
        ($e).map_err(|e| match e {
            async_ssh2::Error::SSH2(e2) => Error::new(std::io::ErrorKind::Other, e2.message()),
            async_ssh2::Error::Io(e) => e,
        })?
    };
}

/// This is needed to implement our complex JSON/length-delimited codec type
/// for the transport layer, since due to error E0225 we can't express the
/// Box<dyn T> trait constraint on more than one trait at a time.
trait AsyncReadAndWrite: AsyncRead + AsyncWrite + Unpin {}

impl AsyncReadAndWrite for TcpStream {}
impl AsyncReadAndWrite for async_ssh2::Channel {}

/// The type that defines our client/server communication. We use JSON to
/// encode our messages via Serde, on top of a length-delimited codec because
/// Serde needs it, on a transport that is abstracted through a Box so that we
/// can use either an SSH connection or a raw TCP connection (or other
/// transports if they're added) as needed.
type HubTransport = SerdeFramed<
    CodecFramed<Box<dyn AsyncReadAndWrite>, LengthDelimitedCodec>,
    DisplayMessage,
    ClientHelloMessage,
    Json<DisplayMessage, ClientHelloMessage>,
>;

impl ClientConfiguration {
    pub async fn connect(&self) -> Result<HubTransport, Error> {
        if let Some(sshcfg) = self.ssh.as_ref() {
            let mut sess = tryssh!(async_ssh2::Session::new());

            // NB this is a non-async TcpStream.connect() so it will block the thread!
            let transport = StdTcpStream::connect((self.hub_host.as_ref(), sshcfg.ssh_port))?;
            tryssh!(sess.set_tcp_stream(transport));

            tryssh!(sess.handshake().await);
            tryssh!(
                sess.userauth_pubkey_file(
                    sshcfg.user.as_ref(),
                    None, // pubkey path; inferred
                    Path::new(&sshcfg.private_key_path),
                    None, // passphrase: assume passwordlessness
                )
                .await
            );

            Ok(Self::wrap_transport(tryssh!(
                sess.channel_direct_tcpip("localhost", self.hub_port, None)
                    .await
            )))
        } else {
            Ok(Self::wrap_transport(
                TcpStream::connect((self.hub_host.as_ref(), self.hub_port)).await?,
            ))
        }
    }

    fn wrap_transport<T: AsyncReadAndWrite + 'static>(transport: T) -> HubTransport {
        let ld = CodecFramed::new(
            Box::new(transport) as Box<dyn AsyncReadAndWrite>,
            LengthDelimitedCodec::new(),
        );
        SerdeFramed::new(ld, Json::default())
    }
}

pub fn main_cli(opts: super::ClientCommand) -> Result<(), Error> {
    openssl_probe::init_ssl_cert_env_vars();

    // Parse the configuration.

    let config: ClientConfiguration = confy::load("rc-stickynote-client")?;

    // If requested, let's get into the background. Do this before any
    // other thread-y operations.

    if opts.daemonize {
        // TODO: files in /var/run, etc? The idea is to lauch this process as
        // an unprivleged user.
        let pid_path: PathBuf = ["rc-stickynote-displayer.pid"].iter().collect();
        let stdio_path: PathBuf = ["rc-stickynote-displayer.log"].iter().collect();
        let stdio_handle = File::create(&stdio_path)?;

        let dconfig = Daemonize::new()
            .pid_file(&pid_path)
            .stdout(stdio_handle.try_clone()?)
            .stderr(stdio_handle);

        if let Err(e) = dconfig.start() {
            return Err(Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    }

    // The actual renderer operates in its own thread since the I/O can be slow
    // and we don't want to block the async runtime.
    let cloned_config = config.clone();
    let (sender, receiver) = channel();
    thread::spawn(move || renderer_thread(cloned_config, receiver));

    let mut rt = Runtime::new()?;

    // Ready to start the main event loop

    rt.block_on(async {
        // How often to wake up this thread if no other events are going
        // on.
        let mut wakeup_interval = time::interval(Duration::from_millis(60_000));

        // the last time something happened with the hub connection.
        let mut last_hub_update = time::Instant::now();

        // if there's a hub problem, wait this long to retry connecting.
        let hub_retry_duration = Duration::from_millis(180_000);

        // How often to redraw the display even if nothing seems to be going on.
        // This will update the clock, etc.
        let redraw_duration = Duration::from_millis(600_000);

        // the last time we redrew the display (approximately, since that's
        // done in another thread and takes nontrivial time).
        let mut last_redraw = time::Instant::now();

        // do we need to redraw even if redraw_duration hasn't elapsed?
        let mut need_redraw = true;

        let mut display_data = DisplayData::new()?;
        let mut connection = ServerConnection::default();

        loop {
            // `select` on various things that might motivate us to update the
            // display.

            select! {
                // New message from the hub.
                msg = connection.get_next_message(&config).fuse() => {
                    last_hub_update = time::Instant::now();
                    need_redraw = true;

                    match msg {
                        Ok(m) => {
                            display_data.update_from_message(m);
                        },

                        Err(err) => {
                            // Note that we do *not* instantly reset `connection`,
                            // because otherwise we just keep on trying to connect
                            // over and over again. If the hub is just totally
                            // down, insistently trying isn't going to help.
                            println!("hub connection failed: {}", err);
                            display_data.update_for_no_connection();
                        }
                    }
                }

                // Time has passed since the last wakeup interval tick.
                _ = wakeup_interval.tick().fuse() => {}
            }

            let now = time::Instant::now();

            // Housekeeping: how's the hub connection looking? If the connection is
            // happy, we're content to just sit and wait -- update messages might
            // not arrive for *days*. But if the connection has problems, retry if
            // the time is right.

            if connection.is_failed() && now.duration_since(last_hub_update) > hub_retry_duration {
                display_data.update_for_no_connection();
                println!("hub error and delay elapsed; attempting to reconnect ...");
                connection = ServerConnection::default();
            }

            // Trigger a draw?

            if need_redraw || now.duration_since(last_redraw) > redraw_duration {
                if let Err(e) = sender.send(display_data.clone()) {
                    // Yikes, this is bad. We don't want to exit the program so ...
                    // just print the error and ignore it. Not much else we can do.
                    // (We could try sending a message to the hub?)
                    println!("display thread died?! {}", e);
                }

                need_redraw = false;
                last_redraw = now;
            }
        }
    })
}

enum ServerConnection {
    Initializing,
    Open(HubTransport),
    Failed,
}

impl Default for ServerConnection {
    fn default() -> Self {
        ServerConnection::Initializing
    }
}

impl ServerConnection {
    fn is_failed(&self) -> bool {
        match self {
            ServerConnection::Failed => true,
            _ => false,
        }
    }

    async fn get_next_message(
        &mut self,
        config: &ClientConfiguration,
    ) -> Result<DisplayMessage, Error> {
        loop {
            match self {
                ServerConnection::Initializing => {
                    // Note: cannot use ?-syntax here since we need to ensure that we set
                    // self to the Failed state is anything goes wrong.

                    let mut hub_comms = match config.connect().await {
                        Ok(c) => c,

                        Err(e) => {
                            *self = ServerConnection::Failed;
                            return Err(e);
                        }
                    };

                    if let Err(e) = hub_comms
                        .send(ClientHelloMessage::Display(DisplayHelloMessage {}))
                        .await
                    {
                        *self = ServerConnection::Failed;
                        return Err(e);
                    }

                    *self = ServerConnection::Open(hub_comms);
                }

                ServerConnection::Open(ref mut hub_comms) => {
                    return match hub_comms.try_next().await {
                        Ok(Some(m)) => {
                            println!("msg: {:?}", m);
                            Ok(m)
                        }

                        Ok(None) => {
                            *self = ServerConnection::Failed;

                            Err(Error::new(std::io::ErrorKind::Other, "hub connection died"))
                        }

                        Err(err) => {
                            *self = ServerConnection::Failed;

                            Err(err)
                        }
                    };
                }

                ServerConnection::Failed => {
                    return futures::future::pending().await;
                }
            }
        }
    }
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

    let ago_formatter = timeago::Formatter::new();

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

        // Update the "local" bits.

        dd.update_local()?;

        // Render into the buffer.

        {
            backend.clear_buffer(Backend::WHITE)?;
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

            fn draw6x8inverted(
                buf: &mut <Backend as DisplayBackend>::Buffer,
                s: &str,
                x: i32,
                y: i32,
            ) {
                buf.draw(
                    Font6x8::render_str(s)
                        .style(Style {
                            fill_color: Some(Backend::BLACK),
                            stroke_color: Some(Backend::WHITE),
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

            // "The Innovation Scientist is ..." text

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

            // The actual status message

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

            // "updated at ..." to go with the status message

            let y = y + delta + 4;

            let msg = format!(
                "updated at {} (more than {})",
                dd.person_is_timestamp
                    .with_timezone(&dd.now.timezone())
                    .format("%I:%M %p"),
                ago_formatter.convert_chrono(dd.person_is_timestamp, dd.now)
            );
            let x = 382 - 6 * (msg.len() as i32);
            draw6x8(buffer, &msg, x, y);

            // Footer and IP address

            let y = 630;
            let delta = 9;

            buffer.draw(
                Rectangle::new(Coord::new(0, y), Coord::new(383, y + delta))
                    .fill(Some(Backend::BLACK)),
            );

            draw6x8inverted(buffer, "https://github.com/pkgw/rc-stickynote", 2, y + 1);

            let x = 382 - 6 * (dd.ip_addr.len() as i32);
            draw6x8inverted(buffer, &dd.ip_addr, x, y + 1);
        }

        // https://www.waveshare.com/wiki/E-Paper_Driver_HAT:
        //
        // "Question: Why my e-paper has ghosting problem after working for
        // some days? Answer: Please set the e-paper to sleep mode or
        // disconnect it if you needn't refresh the e-paper but need to power
        // on your development board or Raspberry Pi for long time. Otherwise,
        // the voltage of panel keeps high and it will damage the panel."
        //
        // The above is why we wake up and sleep the device.
        //
        // Further, keep in mind that on the actual device, showing the buffer
        // takes more than 10 seconds!
        //
        // In principle we could try to be smart and have a timer for sleeping
        // the device to avoid multiple cycles during rapid-fire updates, but
        // that seems like overkill.

        backend.wake_up_device()?;
        backend.show_buffer()?;
        backend.sleep_device()?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct DisplayData {
    // Digested from DisplayMessage:
    pub person_is: String,
    pub person_is_timestamp: DateTime<Utc>,

    // "Local" values determined without the hub:
    pub now: DateTime<Local>,
    pub ip_addr: String,
}

impl DisplayData {
    fn new() -> Result<Self, std::io::Error> {
        let mut dd = DisplayData {
            now: Local::now(),
            person_is: "[connecting to hub...]".to_owned(),
            person_is_timestamp: Utc::now(),
            ip_addr: "".to_owned(),
        };
        dd.update_local()?;
        Ok(dd)
    }

    fn update_from_message(&mut self, msg: DisplayMessage) {
        self.person_is = msg.person_is;
        self.person_is_timestamp = msg.person_is_timestamp;
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

    fn update_for_no_connection(&mut self) {
        // TODO: should preserve the person_is message since it may
        // have contained useful information.
        self.person_is = "[cannot connect to hub!]".to_owned();
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

    openssl_probe::init_ssl_cert_env_vars();

    let config: ClientConfiguration = confy::load("rc-stickynote-client")?;
    let mut rt = Runtime::new()?;

    rt.block_on(async {
        let mut hub_comms = config.connect().await?;

        hub_comms
            .send(ClientHelloMessage::PersonIsUpdate(
                PersonIsUpdateHelloMessage {
                    person_is: opts.status,
                    timestamp: Utc::now(),
                },
            ))
            .await?;
        Ok(())
    })
}
