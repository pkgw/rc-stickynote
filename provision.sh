#! /usr/bin/env bash
# Copyright 2020 Peter Williams
# Licensed under the MIT License.

set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

# XXX We want this update/install stage to *also* route through blobman so
# that it can, in principle, run while fully disconnected. But that's
# something to tackle Later.

sudo apt-get -y update
sudo apt-get -y install \
        bsdtar \
        coreutils \
        curl \
        debootstrap \
        dosfstools \
        file \
        git \
        grep \
        kmod \
        libcap2-bin \
        parted \
        rsync \
        qemu-user-static \
        quilt \
        udev \
        vim \
        xxd \
        xz-utils \
        zerofree \
        zip
sudo apt-get clean
sudo rm -rf /var/lib/apt/lists/*
