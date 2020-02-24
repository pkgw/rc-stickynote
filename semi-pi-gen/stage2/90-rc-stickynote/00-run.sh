#! /bin/bash
#
# Custom setup for the rc-stickynote application.

set -xeuo pipefail

local_dir="$(cd $BASE_DIR/../local && pwd)"
pi_user_home="$ROOTFS_DIR/home/$FIRST_USER_NAME"
rust_bin_dir="$(cd $BASE_DIR/../target/armv7-unknown-linux-gnueabihf/release && pwd)"

# The key binary:

mkdir -p "$ROOTFS_DIR/usr/local/bin"
cp $rust_bin_dir/rc_stickynote_displayer "$ROOTFS_DIR/usr/local/bin/"

# SSH key for connecting to the Hub server:

mkdir -p "$pi_user_home/.ssh"
cp $local_dir/stickynote_*_key* "$pi_user_home"/.ssh
chmod 600 "$pi_user_home"/.ssh/stickynote_*

on_chroot <<EOF
chown -R $FIRST_USER_NAME:$FIRST_USER_NAME /home/$FIRST_USER_NAME/.ssh
EOF

# config file for the client:

mkdir -p "$pi_user_home/.config/rc-stickynote-client"
cp $local_dir/client-config.toml "$pi_user_home/.config/rc-stickynote-client/rc-stickynote-client.toml"

on_chroot <<EOF
chown -R $FIRST_USER_NAME:$FIRST_USER_NAME /home/$FIRST_USER_NAME/.config
EOF

# init scripts. The `housekeeping` one clears the display upon poweroff, and can
# show IP addresses with its `status` command.

mkdir -p "$ROOTFS_DIR/etc/init.d"
cp assets/rc-stickynote-housekeeping "$ROOTFS_DIR/etc/init.d"
cp assets/rc-stickynote-displayer "$ROOTFS_DIR/etc/init.d"

on_chroot <<EOF
update-rc.d rc-stickynote-housekeeping defaults
update-rc.d rc-stickynote-displayer defaults
EOF
