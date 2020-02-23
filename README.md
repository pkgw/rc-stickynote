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


## Software

There are two software components: a “hub” that runs on a persistent server,
and a “display client” that runs on the RPi. Both are written in
[Rust](https://rust-lang.org/). The Pi needs to be able to SSH into the hub
server.


## Step 1: Set up builder VM

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


## Step 2: Configuration

Various configuration files need to be set up in the directory `local/`.

1. Create an SSH keypair that has no protective passphrase. This will be used
   by the Pi to connect to the Hub. Run:
   ```
   ssh-keygen -t ed25519 -f local/stickynote_ed25519_key
   ```
2. Configure the client to talk to the hub. Copy
   `local/client-config.example.toml` to `local/client-config.toml` and
   customize as appropriate for your hub setup, as described in comments in
   that file.
3. Configure the Raspberry Pi OS image build. Copy
   `local/pi-gen-config.example` to `local/pi-gen-config` and customize as
   desired, as described in comments in that file.


## Step 3: Building the Software

Building the software requires a Rust toolchain and the Rust
[cross](https://github.com/rust-embedded/cross) tool.

To build the hub, run:

```
cargo build --bin rc_stickynote_hub --release
```

If your server runs an OS that's not fully compatible with your build machine
(e.g., it runs an older version of glibc), you can cross-compile it if you'd
like:

```
cross build --target x86_64-unknown-linux-musl --release
```

To cross-compile the display client for the RPi, run:

```
cross build --target armv7-unknown-linux-gnueabihf --release
```


## Step 4: Build the RPi OS image

We build the Raspberry Pi OS image using scripts derived from the
[pi-gen][pi-gen] tool used for the official RPi images. The forked scrips live
in the `semi-pi-gen/` directory.

[pi-gen]: https://github.com/RPi-Distro/pi-gen

To build the image, run:

```
vagrant ssh -c "cd /vagrant/semi-pi-gen && sudo STAGE_LIST='stage0 stage1 stage2' ./build.sh"
```

This will output the image file in `semi-pi-gen/deploy/YYYY-MM-DD-rc-stickynote-lite.img`,
where the `YYYY-MM-DD` corresponds to today’s date.

To rebuild a new image with updated configuration or stickynote executables,
you can modify the `STAGE_LIST` in the above command to contain just `stage2.


## Testing: Simulator Client

To run a “simulator” version of the client that uses
[SDL](https://www.libsdl.org/) to draw graphics to a window rather than the
EPD, run:

```
cd displayer && cargo run --no-default-features --features=simulator -- client
```

This program will require a client configuration file, which should be placed
in `~/.config/rc-stickynote-client/rc-stickynote-client.toml`. This is the
same file format as used in `local/client-config.toml`.


## Testing: Checking the RPi OS image

To mount the RPi OS image on your (Linux) machine and poke around its
filesystem, you can mount it with a loopback device. The only tricky part is
that the image file is a partitioned disk image, not just a single filesystem
partition, so you need to be able to tell the mount program the right offset
into the disk image to find the filesystem.

The `pi-gen` build process outputs this information while creating the image. It
will emit some output that looks like this:

```
[19:26:46] Begin /vagrant/semi-pi-gen/export-image/prerun.sh
/boot: offset 4194304, length 268435456
/:     offset 272629760, length 1635778560
```

In the above, the number 272629760 is the offset we want to know.

Alternatively, you can use `fdisk`. If you run `fdisk -lu /path/to/stickynote.img`, the
output will look something like:

```
Disk stickynote.img: 1.8 GiB, 1908408320 bytes, 3727360 sectors
Units: sectors of 1 * 512 = 512 bytes
Sector size (logical/physical): 512 bytes / 512 bytes
I/O size (minimum/optimal): 512 bytes / 512 bytes
Disklabel type: dos
Disk identifier: 0x8319fb68

Device          Boot  Start     End Sectors  Size Id Type
stickynote.img1        8192  532479  524288  256M  c W95 FAT32 (LBA)
stickynote.img2      532480 3727359 3194880  1.5G 83 Linux
```

In this example, the blocksize is the 512 in the `Units:` line, and the offset
in blocks is the "start" value, 532480, in the final output line. The offset
in bytes is the product of these two numbers, `512 * 532480 = 272629760`.

Once you have the offset, mounting the image is as simple as:

```
sudo mount -o loop,offset=$OFFSET /path/to/stickynote.img /tmp/img
```
