# Raspberry Pi Radio-Controlled Sticky Note

This repository contains the software component of a project to show custom
messages on a Raspberry Pi (RPi) hooked up to an e-Print display (EPD),
intended to act as a radio-controlled (RC) sticky note on an office door.

An important aspect of this project is that it is a testbed for using the
[blobman][blobman] framework to construct the necessary software in as
reproducible a fashion as can be managed.

[blobman]: https://github.com/pkgw/blobman/


## Hardware

This project is currently executed on a
[Raspberry Pi 4 Model B](https://www.raspberrypi.org/products/raspberry-pi-4-model-b/),
which turns out to be way overpowered for this application.

The EPD is a 680×374 pixel, 7.5", black-and-white display from
[Waveshare](https://www.waveshare.com/product/displays/e-paper.htm). This
particular model seems not to be available anymore — instead there are options
with the same resolution but three colors, or still monochrome but at higher
resolution.


## Builder VM

In order for this project to work, we need to create a custom Raspberry Pi OS
image that will be flashed onto an SD card. Creating this image requires some
low-level custom Linux-y work, so we do the operations inside a Vagrant
virtual machine (“box”) for reproducibility.

The basis for the builder box is the 20200116.1 version of the
`ubuntu/bionic64` Vagrant box. This can be initialized reproducibly with:

```
blobman provide bionic-server-cloudimg-amd64-vagrant.box
vagrant box add --name ubuntu_bionic64_20200116.1 bionic-server-cloudimg-amd64-vagrant.box
```

Running `vagrant up` will initialize and provision the builder box. **TODO**:
the provisioning uses Apt commands and therefore touches the network! The goal
is to get blobman infrastructure such that we can provision the box even when
running fully offline, but we're not there yet.


## Building the RPi OS Image

We actually build the Raspberry Pi OS image using scripts derived from the
[pi-gen][pi-gen] tool used for the official RPi images.

[pi-gen]: https://github.com/RPi-Distro/pi-gen


## Building the Software

There are two software components: a “hub” that runs on a persistent server,
and a “display client” that runs on the RPi. Both are written in
[Rust](https://rust-lang.org/).

To build the hub, run:

```
cargo build --bin hub --release
```

To cross-compile the display client for the RPi, install the Rust
[cross](https://github.com/rust-embedded/cross) tool and run:

```
cross build --target armv7-unknown-linux-gnueabihf --release
```

To run a “simulator” version of the client that uses
[SDL](https://www.libsdl.org/) to draw graphics to a window rather than the
EPD, run:

```
cd displayer && cargo run --no-default-features --features=simulator -- client ../sample-client-config.toml
```

Here, `../sample-client-config.toml` is the path to a configuration file for
the client.
