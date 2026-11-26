use ffmpeg_next::{Error, Packet, codec::decoder::Opened, frame::Frame};
use log::warn;

use crate::benchmark::{self, Stage};

pub(crate) struct DecodeRecovery {
    stage: Stage,
    track: &'static str,
    channel_id: i32,
    errors: u64,
}

impl DecodeRecovery {
    pub(crate) fn new(stage: Stage, track: &'static str, channel_id: i32) -> Self {
        Self {
            stage,
            track,
            channel_id,
            errors: 0,
        }
    }

    pub(super) fn errors(&self) -> u64 {
        self.errors
    }

    pub(crate) fn send(&mut self, decoder: &mut Opened, packet: &Packet) -> Result<(), Error> {
        match benchmark::measure(self.stage, || decoder.send_packet(packet)) {
            Err(Error::InvalidData) => {
                self.record_error();
                Ok(())
            }
            result => result,
        }
    }

    pub(super) fn start_drain(&mut self, decoder: &mut Opened) -> Result<(), Error> {
        match benchmark::measure(self.stage, || decoder.send_eof()) {
            Err(Error::Eof) => Ok(()),
            Err(Error::InvalidData) => {
                self.record_error();
                Ok(())
            }
            result => result,
        }
    }

    pub(crate) fn receive(
        &mut self,
        decoder: &mut Opened,
        frame: &mut Frame,
    ) -> Result<bool, Error> {
        loop {
            match benchmark::measure_success(self.stage, || decoder.receive_frame(frame)) {
                Ok(()) => return Ok(true),
                Err(Error::Eof) => return Ok(false),
                Err(Error::Other { errno }) if errno == ffmpeg_next::error::EAGAIN => {
                    return Ok(false);
                }
                Err(Error::InvalidData) => self.record_error(),
                Err(error) => return Err(error),
            }
        }
    }

    fn record_error(&mut self) {
        if self.errors == 0 {
            warn!(channel = self.channel_id;
                "{} decoder reported invalid data; continuing with the next decodable frame",
                self.track
            );
        }
        self.errors += 1;
    }
}
