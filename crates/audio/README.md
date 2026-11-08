# ff-audio

Internal workspace library for PCM gain, loudness measurement and correction,
stereo-linked dynamics, true-peak limiting and level metering. There are no
FFmpeg, device, GUI or runtime dependencies. The crate is not published yet.

`PlanarBlock` borrows equally sized `f32` channel slices with an explicit speaker
layout, without copying or allocating. Gain processing supports arbitrary channel
counts. Current loudness/dynamics/limiter algorithms support left/right stereo;
`StereoBlock::from_planar` rejects other layouts. More channels in a block do not
implicitly enable surround loudness weighting or channel linking.

Each independent audio track owns its processor instances. Construct processors
for the source sample rate and recreate them when that rate changes. Neither
sample-rate conversion nor track scheduling is performed here. Lookahead inputs
are contiguous future source samples, not additional samples to consume. The
caller buffers matching audio/video latency and decides when to begin a new
segment while retaining processor history.

The engine contains thin adapters for normalized writable FFmpeg audio frames.
Its public control/metrics types remain available through `ff-engine`. Codec I/O,
A/V buffering, timestamps, source transitions and preview/device output stay in
that crate. Source paths and playout configuration are not part of this API.

```rust
use ff_audio::{AudioEffect, AudioEffectsControl, buffer::StereoBlock, volume::GainEffect};

let mut left = [0.5_f32; 480];
let mut right = [0.25_f32; 480];
let mut block = StereoBlock::new(&mut left, &mut right)?;
let control = AudioEffectsControl::new(0.5)?;
let mut gain = GainEffect::new(control, 48_000);
gain.process(&mut block);
# Ok::<(), anyhow::Error>(())
```

Run `cargo test -p ff-audio --lib` for codec-independent tests and
`cargo bench -p ff-audio --bench audio_dynamics` for processing benchmarks.
`ff-engine`'s `libc_audio` benchmark additionally measures the FFmpeg adapter.
