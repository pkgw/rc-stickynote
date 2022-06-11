//! Display backend for the Waveshare 7.5-inch e-Print Display.
//!
//! Pin assignments: see
//! <https://www.waveshare.com/wiki/Template:Raspberry_Pi_Guides_for_SPI_e-Paper>.
//! Apparently we're in the "BCM2835" side of things.

use embedded_graphics::pixelcolor::BinaryColor;
use epd_waveshare::{
    color::Color as WaveshareColor,
    epd7in5::{Display7in5, Epd7in5},
    graphics::Display,
    prelude::*,
};
use linux_embedded_hal::{
    gpio_cdev::{self, LineRequestFlags},
    spidev::{self, SpidevOptions},
    CdevPin, Delay, Spidev,
};
use std::io::Error;

use super::DisplayBackend;

pub struct Epd7in5Backend {
    spi: Spidev,
    epd7in5: Epd7in5<Spidev, CdevPin, CdevPin, CdevPin, CdevPin, Delay>,
    display: Display7in5,
    delay: Delay,
}

fn binary_color_to_waveshare(c: BinaryColor) -> WaveshareColor {
    match c {
        BinaryColor::On => WaveshareColor::Black,
        BinaryColor::Off => WaveshareColor::White,
    }
}

impl DisplayBackend for Epd7in5Backend {
    type Color = BinaryColor;
    type Buffer = Display7in5;

    const BLACK: BinaryColor = BinaryColor::On;
    const WHITE: BinaryColor = BinaryColor::Off;

    fn open() -> Result<Self, Error> {
        // This is all copied from the epd-waveshare 7in5 example.
        // TODO: remove .expect()s

        let mut spi = Spidev::open("/dev/spidev0.0")?;
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(4_000_000)
            .mode(spidev::SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&options)?;

        // TO CHECK: we used to have the Chip Select pin as pin 8,
        // but based on https://github.com/caemor/epd-waveshare/issues/42,
        // I think we need to set it to some random other pin, because
        // the SPI layer manages CS for us ... or something.
        let mut chip = gpio_cdev::Chip::new("/dev/gpiochip0").unwrap();
        let line = chip.get_line(23).unwrap(); // unused pin????
        let cs_handle = line
            .request(LineRequestFlags::OUTPUT, 1, "rc_stickynote_displayer")
            .unwrap();
        let cs = CdevPin::new(cs_handle).unwrap();

        // TO CHECK: can we safely skip this?????? Sleeps also remove from
        // subsequent stanzas.
        //
        // See https://github.com/rust-embedded/rust-sysfs-gpio/issues/5 --
        // after the CdevPin is exported, there is a small window before the RPi
        // udev system changes permissions on the created device file. If we try
        // to set the direction before this window elapses, we fail with EACCES
        // when run as non-root. We're only booting up infrequently, so just
        // hardcode a delay. sleep(Duration::from_millis(750));

        cs.set_value(1).expect("CS value set to 1");

        let line = chip.get_line(24).unwrap(); // Busy pin
        let busy_handle = line
            .request(LineRequestFlags::INPUT, 0, "rc_stickynote_displayer")
            .unwrap();
        let busy = CdevPin::new(busy_handle).unwrap();

        let line = chip.get_line(25).unwrap(); // DC pin
        let dc_handle = line
            .request(LineRequestFlags::OUTPUT, 1, "rc_stickynote_displayer")
            .unwrap();
        let dc = CdevPin::new(dc_handle).unwrap();

        let line = chip.get_line(17).unwrap(); // RST pin
        let rst_handle = line
            .request(LineRequestFlags::OUTPUT, 1, "rc_stickynote_displayer")
            .unwrap();
        let rst = CdevPin::new(rst_handle).unwrap();

        let mut delay = Delay {};
        let epd7in5 = Epd7in5::new(&mut spi, cs, busy, dc, rst, &mut delay)?;
        let mut display = Display7in5::default();

        display.set_rotation(DisplayRotation::Rotate270);

        Ok(Epd7in5Backend {
            spi,
            epd7in5,
            display,
            delay,
        })
    }

    fn clear_buffer(&mut self, color: Self::Color) -> Result<(), Error> {
        self.display.clear_buffer(binary_color_to_waveshare(color));
        Ok(())
    }

    fn get_buffer_mut(&mut self) -> &mut Self::Buffer {
        &mut self.display
    }

    fn show_buffer(&mut self) -> Result<(), Error> {
        self.epd7in5
            .update_frame(&mut self.spi, &self.display.buffer(), &mut self.delay)?;
        self.epd7in5.display_frame(&mut self.spi, &mut self.delay)?;
        Ok(())
    }

    fn clear_display(&mut self) -> Result<(), Error> {
        self.epd7in5.clear_frame(&mut self.spi, &mut self.delay)?;
        self.epd7in5.display_frame(&mut self.spi, &mut self.delay)?;
        Ok(())
    }

    fn sleep_device(&mut self) -> Result<(), Error> {
        Ok(self.epd7in5.sleep(&mut self.spi, &mut self.delay)?)
    }

    fn wake_up_device(&mut self) -> Result<(), Error> {
        Ok(self.epd7in5.wake_up(&mut self.spi, &mut self.delay)?)
    }
}
