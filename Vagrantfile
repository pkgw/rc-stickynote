# -*- mode: ruby -*-
# Copyright 2020 Peter Williams.
# Licensed under the MIT License.
#
# This Vagrantfile provisions a Linux VM capable of building a Raspberry Pi OS
# image using a derivative of the `pi-gen` framework.

Vagrant.configure("2") do |config|
  config.vm.box = "ubuntu_bionic64_20200116.1"
  config.vm.provision :shell, path: "provision.sh"
end
