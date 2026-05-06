use crate::rp_hal::hal;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use hal::dma::{Byte, HalfWord, SingleChannel};
use hal::pio::{PIOExt, PIO, Running, StateMachine, StateMachineIndex, Stopped, Tx, Rx, UninitStateMachine};
use mipidsi::interface::{Interface, InterfaceKind};
use rp235x_hal::timer::TimerDevice;
use super::DmaStreamer;

pub struct SpiPioDmaInterface<
    CS,
    DC,
    P: PIOExt,
    SM1: StateMachineIndex,
    SM2: StateMachineIndex,
    CH1,
    CH2,
    TD: TimerDevice
> {
    streamer: DmaStreamer<CH1, CH2>,
    mode: Option<PioMode<P, SM1, SM2>>,
    cs: CS,
    dc: DC,
    timer: crate::hal::Timer<TD>,
}

enum PioMode<P: PIOExt, SM1: StateMachineIndex, SM2: StateMachineIndex> {
    Byte(
        (
            PioCont<P, SM1, Byte, Running>,
            PioCont<P, SM2, HalfWord, Stopped>
        )
    ),
    HalfWord(
        (
            PioCont<P, SM1, Byte, Stopped>,
            PioCont<P, SM2, HalfWord, Running>
        )
    ),
}

struct PioCont<P: PIOExt, SM: StateMachineIndex, TxSz, State> {
    sm: StateMachine<(P, SM), State>,
    tx: Tx<(P, SM), TxSz>,
    rx: Rx<(P, SM)>,
}

impl<CS, DC, P, SM1, SM2, CH1, CH2, TD> SpiPioDmaInterface<CS, DC, P, SM1, SM2, CH1, CH2, TD>
where
    P: PIOExt,
    SM1: StateMachineIndex,
    SM2: StateMachineIndex,
    CS: OutputPin,
    DC: OutputPin,
    CH1: SingleChannel,
    CH2: SingleChannel,
    TD: TimerDevice,
{
    pub fn new(
        clock_divider: (u16, u8),
        mut cs: CS,
        dc: DC,
        pio: &mut PIO<P>,
        sm1: UninitStateMachine<(P, SM1)>,
        sm2: UninitStateMachine<(P, SM2)>,
        clk: u8,
        tx: u8,
        streamer: DmaStreamer<CH1, CH2>,
        timer: crate::hal::Timer<TD>,
    ) -> Self {
        cs.set_high().ok();

        let prog = pio_proc::pio_asm!(".side_set 1", "out pins, 1 side 0", "nop side 1");

        let installed = pio.install(&prog.program).unwrap();
        let (mut sm8, rx8, tx8) = hal::pio::PIOBuilder::from_installed_program(installed)
            .out_pins(tx, 1)
            .side_set_pin_base(clk)
            .autopull(true)
            .pull_threshold(8)
            .out_shift_direction(hal::pio::ShiftDirection::Left)
            .in_shift_direction(hal::pio::ShiftDirection::Left)
            .buffers(hal::pio::Buffers::OnlyTx)
            .clock_divisor_fixed_point(clock_divider.0, clock_divider.1)
            .build(sm1);
        sm8.set_pindirs([(tx, hal::pio::PinDir::Output)]);
        sm8.set_pindirs([(clk, hal::pio::PinDir::Output)]);

        let installed = pio.install(&prog.program).unwrap();
        let (mut sm16, rx16, tx16) = hal::pio::PIOBuilder::from_installed_program(installed)
            .out_pins(tx, 1)
            .side_set_pin_base(clk)
            .autopull(true)
            .pull_threshold(16)
            .out_shift_direction(hal::pio::ShiftDirection::Left)
            .in_shift_direction(hal::pio::ShiftDirection::Left)
            .buffers(hal::pio::Buffers::OnlyTx)
            .clock_divisor_fixed_point(clock_divider.0, clock_divider.1)
            .build(sm2);
        sm16.set_pindirs([(tx, hal::pio::PinDir::Output)]);
        sm16.set_pindirs([(clk, hal::pio::PinDir::Output)]);

        Self {
            streamer,
            cs,
            dc,
            mode: Some(PioMode::Byte((
                PioCont { sm: sm8.start(), tx: tx8.transfer_size(Byte), rx: rx8 },
                PioCont { sm: sm16, tx: tx16.transfer_size(HalfWord), rx: rx16 },
            ))),
            timer,
        }
    }

    fn to_byte_mode(mode: PioMode<P, SM1, SM2>) -> (PioCont<P, SM1, Byte, Running>, PioCont<P, SM2, HalfWord, Stopped>) {
        match mode {
            PioMode::Byte(p) => p,
            PioMode::HalfWord((b, h)) => (
                PioCont { sm: b.sm.start(), tx: b.tx, rx: b.rx },
                PioCont { sm: h.sm.stop(),  tx: h.tx, rx: h.rx },
            ),
        }
    }

    fn to_halfword_mode(mode: PioMode<P, SM1, SM2>) -> (PioCont<P, SM1, Byte, Stopped>, PioCont<P, SM2, HalfWord, Running>) {
        match mode {
            PioMode::HalfWord(p) => p,
            PioMode::Byte((b, h)) => (
                PioCont { sm: b.sm.stop(),  tx: b.tx, rx: b.rx },
                PioCont { sm: h.sm.start(), tx: h.tx, rx: h.rx },
            ),
        }
    }

    #[inline(always)]
    fn wait_idle(&mut self) {
        let mode = self.mode.as_mut().unwrap();
        
        match mode {
            PioMode::Byte(p) => {
                // 1. Wait for DMA/CPU to finish feeding the TX FIFO
                while !p.0.tx.is_empty() { crate::hal::arch::nop(); }
                // 2. Clear the sticky stall flag BEFORE waiting on it
                p.0.tx.clear_stalled_flag();
                // 3. Wait for the final bits to leave the shift register and stall
                while !p.0.tx.has_stalled() { crate::hal::arch::nop(); }
            },
            PioMode::HalfWord(p) => {
                while !p.1.tx.is_empty() { crate::hal::arch::nop(); }
                p.1.tx.clear_stalled_flag();
                while !p.1.tx.has_stalled() { crate::hal::arch::nop(); }
            },
        }
    }

    #[inline(always)]
    fn send_bytes(&mut self, iter: &mut dyn Iterator<Item = u8>) {
        let (mut b, h) = Self::to_byte_mode(self.mode.take().unwrap());
        b.tx = self.streamer.stream_8b(b.tx, iter);
        self.mode = Some(PioMode::Byte((b, h)));
    }

    #[inline(always)]
    fn send_pixels_u16(&mut self, iter: &mut dyn Iterator<Item = u16>) {
        let (b, mut h) = Self::to_halfword_mode(self.mode.take().unwrap());
        h.tx = self.streamer.stream_16b(h.tx, iter, u16::to_be);
        self.mode = Some(PioMode::HalfWord((b, h)));
    }
}

impl<CS, DC, P, SM1, SM2, CH1, CH2, TD> Interface
    for SpiPioDmaInterface<CS, DC, P, SM1, SM2, CH1, CH2, TD>
where
    P: PIOExt,
    SM1: StateMachineIndex,
    SM2: StateMachineIndex,
    CS: OutputPin,
    DC: OutputPin,
    CH1: SingleChannel,
    CH2: SingleChannel,
    TD: TimerDevice,
{
    type Word = u8;
    type Error = core::convert::Infallible;

    const KIND: InterfaceKind = InterfaceKind::Serial4Line;

    fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        self.wait_idle();
        self.cs.set_low().ok();

        self.dc.set_low().ok();
        self.timer.delay_ns(9000);
        self.send_bytes(&mut core::iter::once(command));

        if !args.is_empty() {
            self.wait_idle();
            self.dc.set_high().ok();
            self.timer.delay_ns(9000);
            self.send_bytes(&mut args.iter().cloned());
        }

        self.wait_idle();
        self.cs.set_high().ok();
        Ok(())
    }

    fn send_pixels<const N: usize>(
        &mut self,
        pixels: impl IntoIterator<Item = [Self::Word; N]>,
    ) -> Result<(), Self::Error> {
        self.wait_idle();
        self.cs.set_low().ok();
        self.dc.set_high().ok();
        self.timer.delay_ns(9000);

        if N == 2 {
            self.send_pixels_u16(
                &mut pixels.into_iter().map(|p| u16::from_be_bytes([p[0], p[1]])),
            );
        } else {
            self.send_bytes(&mut pixels.into_iter().flat_map(|p| p.into_iter()));
        }

        self.wait_idle();
        self.cs.set_high().ok();
        Ok(())
    }

    fn send_repeated_pixel<const N: usize>(
        &mut self,
        pixel: [Self::Word; N],
        count: u32,
    ) -> Result<(), Self::Error> {
        self.wait_idle();
        self.cs.set_low().ok();
        self.dc.set_high().ok();
        self.timer.delay_ns(9000);

        if N == 2 {
            let px = u16::from_be_bytes([pixel[0], pixel[1]]);
            self.send_pixels_u16(&mut core::iter::repeat(px).take(count as usize));
        } else {
            self.send_bytes(
                &mut core::iter::repeat(pixel).take(count as usize).flat_map(|p| p.into_iter()),
            );
        }

        self.wait_idle();
        self.cs.set_high().ok();
        Ok(())
    }
}
