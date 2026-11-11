use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use ff_hls::{MediaPacket, SegmentWriter};
use ffmpeg_next::{Dictionary, Packet, Rational, codec, ffi, format};

use crate::utils::ffmpeg::reference_packet;

pub(super) struct Track {
    pub parameters: codec::Parameters,
    pub time_base: Rational,
}

pub(super) struct TsWriter {
    pub tracks: Vec<[Track; 2]>,
}

impl SegmentWriter<Packet> for TsWriter {
    fn write_segment(
        &mut self,
        path: &Path,
        packets: &[MediaPacket<Packet>],
        rendition: usize,
    ) -> Result<()> {
        let tracks = &self.tracks[rendition];
        ensure!(
            !path.exists(),
            "refusing to overwrite HLS segment {}",
            path.display()
        );

        let temp = path.with_extension("ts.native.tmp");
        let result = (|| -> Result<()> {
            let mut output = format::output_as(&temp, "mpegts")?;

            // Preserve the shared presentation clock exactly, including the 33-bit
            // wrap in PES headers. WebVTT maps to this same clock, without muxdelay.
            // SAFETY: this exclusively owned context has not written its header yet.
            unsafe {
                (*output.as_mut_ptr()).avoid_negative_ts = 0;
                (*output.as_mut_ptr()).max_interleave_delta = 100_000;
                (*output.as_mut_ptr()).max_delay = 0;
            }

            for track in tracks {
                let mut stream = output.add_stream(codec::Id::None)?;

                stream.set_parameters(track.parameters.clone());
                stream.set_time_base(track.time_base);
            }

            let mut options = Dictionary::new();

            options.set("mpegts_copyts", "1");
            options.set("mpegts_flags", "resend_headers+initial_discontinuity");

            let unused = output.write_header_with(options)?;

            ensure!(
                unused.iter().next().is_none(),
                "unused native TS muxer options"
            );
            drop(unused);

            for packet in packets
                .iter()
                .filter(|packet| packet.stream / 2 == rendition)
            {
                let index = packet.stream % 2;
                let mut packet = reference_packet(&packet.payload)?;
                let time_base = output
                    .stream(index)
                    .context("TS stream missing")?
                    .time_base();

                packet.set_stream(index);
                packet.set_position(-1);
                packet.rescale_ts(tracks[index].time_base, time_base);
                packet.write_interleaved(&mut output)?;
            }

            output.write_trailer()?;
            // SAFETY: the writer exclusively owns this live context. Closing via
            // the pointer also clears it, so Output's destructor cannot close twice.
            let closed = unsafe { ffi::avio_closep(&mut (*output.as_mut_ptr()).pb) };

            if closed < 0 {
                return Err(ffmpeg_next::Error::from(closed)).context("closing native HLS segment");
            }

            drop(output);
            fs::rename(&temp, path).with_context(|| format!("publishing {}", path.display()))?;

            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(temp);
        }

        result
    }
}
