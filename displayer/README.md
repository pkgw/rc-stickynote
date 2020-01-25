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
