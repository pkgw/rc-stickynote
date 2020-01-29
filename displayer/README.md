# displayer

This is the program that runs on the Raspberry Pi and actually displays things
on the e-Paper screen!

This crate actually has features. The default feature `waveshare` will include
the crate `epd-waveshare` as a dependency, and build an executable that tries
to send commands to a real Waveshare display using SPI.

The feature `simulator`, which is incompatible with `waveshare`, uses an
SDL2-based simulator instead. This can be used for testing on a standard Linux
machine. Build with:

```
cargo build --no-default-features --features=simulator
```

etc.


## Command-Line Interface

This crate compiles to an executable, `displayer`, that has a git-like
command-line interface with various subcommands. These subcommands are:

- `clear-and-sleep` — clear the display and sleep the device
- `demo-font` — render a TTF or OTF font at various sizes. Some fonts work better
  on monochrome displays than others.
- `show-ips` — print the IPv4 addresses of the machine’s non-loopback network
  interfaces on the display. If no network interfaces have IPv4 addresses, the
  program will sleep and retry for 100 seconds. This makes it suitable to be
  run at bootup so that if your RPi automatically establishes some kind of
  network connection, you can see its addres and know where to SSH to.
