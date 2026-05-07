use alloc::string::String;
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::prelude::{DrawTarget, Point, Primitive, Size};
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
pub mod loading;
pub mod rom_select;
pub struct ListDisplay {
    position: Point,
    item_height: i32,
    item_padding: i32,
    width: i32,
}

impl ListDisplay {
    pub fn new(position: Point, width: i32, item_height: i32, item_padding: i32) -> Self {
        ListDisplay {
            position,
            item_height: item_height,   // Height for each item
            item_padding: item_padding, // Padding between items
            width,
        }
    }

    pub fn draw<D, I, S>(
        &self,
        items: I,
        selected: u8,
        display: &mut D,
    ) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
        I: Iterator<Item = S>,
        S: AsRef<str>
    {
        // Draw each item
        for (index, item) in items.enumerate() {
            let item = item.as_ref();
            let y_offset = index as i32 * (self.item_height + self.item_padding);
            let item_position = self.position + Point::new(0, y_offset);

            let rec_style = if index == selected as usize {
                let mut style = PrimitiveStyle::with_fill(Rgb565::BLACK);
                style.stroke_color = Some(Rgb565::WHITE);
                style.stroke_width = 1;
                style
            } else {
                let mut style = PrimitiveStyle::with_fill(Rgb565::WHITE);
                style.stroke_color = Some(Rgb565::WHITE);
                style.stroke_width = 1;
                style
            };

            let text_style = if index == selected as usize {
                MonoTextStyleBuilder::new()
                    .font(&FONT_6X9)
                    .text_color(Rgb565::WHITE)
                    .build()
            } else {
                MonoTextStyleBuilder::new()
                    .font(&FONT_6X9)
                    .text_color(Rgb565::BLACK)
                    .build()
            };

            Rectangle::new(
                item_position,
                Size::new(self.width as u32, self.item_height as u32),
            )
            .into_styled(rec_style)
            .draw(display)?;

            // Draw text
            Text::with_baseline(
                item,
                item_position + Point::new(5, self.item_height / 2),
                text_style,
                Baseline::Middle,
            )
            .draw(display)?;
        }

        Ok(())
    }
}
