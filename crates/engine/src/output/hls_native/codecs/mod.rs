use ffmpeg_next::{Packet, codec, packet::side_data};

mod aac;
mod bits;
mod h264;
#[cfg(test)]
mod tests;

pub(super) struct Codecs {
    video: Option<String>,
    audio: Option<String>,
}

fn initialization(parameters: &codec::Parameters) -> &[u8] {
    // SAFETY: the borrowed parameters own this immutable allocation for the entire
    // returned slice lifetime. Missing or nonpositive-length data is never sliced.
    unsafe {
        let raw = &*parameters.as_ptr();

        if raw.extradata.is_null() || raw.extradata_size <= 0 {
            return &[];
        }

        std::slice::from_raw_parts(raw.extradata, raw.extradata_size as usize)
    }
}

impl Codecs {
    pub fn new(video: &codec::Parameters, audio: &codec::Parameters) -> Self {
        Self {
            video: (video.id() == codec::Id::H264)
                .then(|| h264::codec(initialization(video)))
                .flatten(),
            audio: (audio.id() == codec::Id::AAC)
                .then(|| aac::codec(initialization(audio)))
                .flatten(),
        }
    }

    pub fn value(&self) -> Option<String> {
        Some(format!("{},{}", self.video.as_ref()?, self.audio.as_ref()?))
    }

    pub fn observe(&mut self, packet: &Packet) -> Option<String> {
        if self.video.is_some() && self.audio.is_some() {
            return None;
        }

        let video = packet.stream().is_multiple_of(2);
        let missing = if video {
            self.video.is_none()
        } else {
            self.audio.is_none()
        };

        if !missing {
            return None;
        }

        let extra = packet
            .side_data()
            .find(|data| data.kind() == side_data::Type::NewExtraData);
        let found = if video {
            extra.and_then(|data| h264::codec(data.data())).or_else(|| {
                packet
                    .is_key()
                    .then(|| h264::codec(packet.data().unwrap_or_default()))
                    .flatten()
            })
        } else {
            // Raw AAC payloads do not reliably identify implicit SBR/PS. Never
            // infer the object type from an encoder name, profile hint or ADTS alone.
            extra.and_then(|data| aac::codec(data.data()))
        }?;

        if video {
            self.video = Some(found);
        } else {
            self.audio = Some(found);
        }

        self.value()
    }
}
