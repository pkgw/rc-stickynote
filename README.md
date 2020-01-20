# Raspberry Pi eInk Message Panel Project

The software component of a project to show custom messages on a Raspberry Pi
hooked up to an e-Ink display.

An important aspect of this project is that it is a testbed for using the
[blobman][blobman] framework to construct the necessary software in as
reproducible a fashion as can be managed.

[blobman]: https://github.com/pkgw/blobman/


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
