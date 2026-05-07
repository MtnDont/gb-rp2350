use core::convert::Infallible;

use alloc::string::String;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X12, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    prelude::{DrawTarget, Point, RgbColor, WebColors},
    text::{Baseline, Text},
};
use embedded_hal::digital::InputPin;
use rp235x_hal::timer::TimerDevice;

use crate::{util::LimitedViewList, RENDER_HEIGHT, RENDER_WIDTH};

use super::ListDisplay;

#[inline(always)]
pub fn select_rom<'a, D: DrawTarget<Color = Rgb565>, TD: TimerDevice>(
    display: &mut D,
    rom_list: &[(String, String)],
    mut _timer: crate::hal::Timer<TD>,
    up_button: &'a mut dyn InputPin<Error = Infallible>,
    down_button: &'a mut dyn InputPin<Error = Infallible>,
    select_button: &'a mut dyn InputPin<Error = Infallible>,
) -> Result<usize, D::Error> {
    let mut selected_rom = 0u8;
    let mut button_clicked = false;

    display.clear(Rgb565::CSS_GRAY)?;

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X12)
        .text_color(Rgb565::WHITE)
        .build();

    Text::with_baseline(
        "Select Rom:",
        Point::new(0, 7),
        title_style,
        Baseline::Middle,
    )
    .draw(display)?;

    let list = ListDisplay::new(
        Point::new(0, 20),   // Starting position
        RENDER_WIDTH as i32, // Width in pixels
        20,
        5,
    );
    let max_items_to_display = ((RENDER_HEIGHT / (20 + 5)) as usize) - 1;
    let mut items = LimitedViewList::new(
        rom_list,
        max_items_to_display
    );
    list.draw(items.iter().map(|d| d.0.as_str()), 0, display)?;
    loop {
        if up_button.is_low().unwrap() && !button_clicked {
            if selected_rom != 0 {
                selected_rom = selected_rom - 1;
                defmt::info!("up_button Start redraw: {}", selected_rom);
                list.draw(items.iter().map(|d| d.0.as_str()), selected_rom, display)?;
            } else {
                items.prev();
                list.draw(items.iter().map(|d| d.0.as_str()), selected_rom, display)?;
            }
            button_clicked = true;
        }
        if down_button.is_low().unwrap() && !button_clicked {
            if selected_rom + 1 < items.max() as u8 {
                selected_rom = selected_rom + 1;
                defmt::info!("down_button Start redraw: {}", selected_rom);
                list.draw(items.iter().map(|d| d.0.as_str()), selected_rom, display)?;
            } else if (items.len() - items.current_cursor()) > items.max() {
                items.next();
                list.draw(items.iter().map(|d| d.0.as_str()), selected_rom, display)?;
            }
            button_clicked = true;
        }
        if select_button.is_low().unwrap() {
            return Ok(items.current_cursor() + selected_rom as usize);
        }

        if down_button.is_high().unwrap() && up_button.is_high().unwrap() {
            button_clicked = false;
        }
    }
}
