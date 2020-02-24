//! Display backend for the Waveshare 7.5-inch e-Print Display.

use epd_waveshare::{
    color::Color,
    epd7in5::{Display7in5, EPD7in5},
    graphics::Display,
    prelude::*,
};
use linux_embedded_hal::{
    spidev::{self, SpidevOptions},
    sysfs_gpio::Direction,
    Delay, Pin, Spidev,
};
use std::{io::Error, thread::sleep, time::Duration};

use super::DisplayBackend;

pub struct EPD7in5Backend {
    spi: Spidev,
    epd7in5: EPD7in5<Spidev, Pin, Pin, Pin, Pin>,
    display: Display7in5,
}

impl DisplayBackend for EPD7in5Backend {
    type Color = Color;
    type Buffer = Display7in5;

    const BLACK: Color = Color::Black;
    const WHITE: Color = Color::White;

    fn open() -> Result<Self, Error> {
        // This is all copied from the epd-waveshare 7in5 example.
        // TODO: remove .expect()s

        let mut spi = Spidev::open("/dev/spidev0.0")?;
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(4_000_000)
            .mode(spidev::SPI_MODE_0)
            .build();
        spi.configure(&options)?;

        let cs = Pin::new(8); // Chip Select pin
        cs.export().expect("cs export");
        while !cs.is_exported() {}
        // See https://github.com/rust-embedded/rust-sysfs-gpio/issues/5 --
        // after the pin is exported, there is a small window before the
        // RPi udev system changes permissions on the created device file.
        // If we try to set the direction before this window elapses, we
        // fail with EACCES when run as non-root. We're only booting up
        // infrequently, so just hardcode a delay.
        sleep(Duration::from_millis(750));
        cs.set_direction(Direction::Out).expect("CS Direction");
        cs.set_value(1).expect("CS Value set to 1");

        let busy = Pin::new(24); // Busy pin
        busy.export().expect("busy export");
        while !busy.is_exported() {}
        sleep(Duration::from_millis(750)); // see above
        busy.set_direction(Direction::In).expect("busy Direction");

        let dc = Pin::new(25);
        dc.export().expect("dc export");
        while !dc.is_exported() {}
        sleep(Duration::from_millis(750)); // see above
        dc.set_direction(Direction::Out).expect("dc Direction");
        dc.set_value(1).expect("dc Value set to 1");

        let rst = Pin::new(17);
        rst.export().expect("rst export");
        while !rst.is_exported() {}
        sleep(Duration::from_millis(750)); // see above
        rst.set_direction(Direction::Out).expect("rst Direction");
        rst.set_value(1).expect("rst Value set to 1");

        let mut delay = Delay {};
        let epd7in5 = EPD7in5::new(&mut spi, cs, busy, dc, rst, &mut delay)?;
        let mut display = Display7in5::default();

        display.set_rotation(DisplayRotation::Rotate270);

        Ok(EPD7in5Backend {
            spi,
            epd7in5,
            display,
        })
    }

    fn clear_buffer(&mut self, color: Self::Color) -> Result<(), Error> {
        self.display.clear_buffer(color);
        Ok(())
    }

    fn get_buffer_mut(&mut self) -> &mut Self::Buffer {
        &mut self.display
    }

    fn show_buffer(&mut self) -> Result<(), Error> {
        self.epd7in5
            .update_frame(&mut self.spi, &self.display.buffer())?;
        self.epd7in5.display_frame(&mut self.spi)?;
        Ok(())
    }

    fn clear_display(&mut self) -> Result<(), Error> {
        self.epd7in5.clear_frame(&mut self.spi)?;
        self.epd7in5.display_frame(&mut self.spi)?;
        Ok(())
    }

    fn sleep_device(&mut self) -> Result<(), Error> {
        Ok(self.epd7in5.sleep(&mut self.spi)?)
    }
}
