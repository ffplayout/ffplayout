//! Transcode once per equivalent profile. A blocked muxer only loses media
//! from its own mailbox, leaving this encoder and the other destinations live.
use std::{
    sync::{Arc, Mutex, PoisonError, Weak, atomic::Ordering},
    thread::{self, JoinHandle},
};

use anyhow::Result;
use ffmpeg_next::Packet;

use super::{
    Mailbox, POLL, Route, Track, TrackPublication,
    transcode::{DecodeFailure, Profile, TrackEncoder},
};

struct Consumer {
    mailbox: Weak<Mailbox>,
    route: Weak<Route>,
}

// None closes registration before the worker starts exiting.
type Consumers = Arc<Mutex<Option<Vec<Consumer>>>>;

pub(super) struct Group {
    profile: Profile,
    muxer: String,
    actions: Vec<bool>,
    mailbox: Arc<Mailbox>,
    consumers: Consumers,
    pub(super) publication: TrackPublication,
    thread: JoinHandle<()>,
}

impl Group {
    pub(super) fn start(
        route: &Route,
        tracks: Vec<Track>,
        plan: Vec<Option<String>>,
    ) -> Result<Self> {
        let mailbox = Mailbox::new();
        let consumers = Arc::new(Mutex::new(Some(Vec::new())));
        let publication = TrackPublication::default();
        let profile = route.profile.clone();
        let global_header = route.muxer == "flv";
        let actions: Vec<_> = plan.iter().map(Option::is_some).collect();
        let thread = thread::Builder::new()
            .name("ingest-transcode".into())
            .spawn({
                let mailbox = mailbox.clone();
                let consumers = consumers.clone();
                let publication = publication.clone();
                let profile = profile.clone();
                let actions = actions.clone();

                move || {
                    let result = run(
                        &mailbox,
                        &consumers,
                        &publication,
                        &tracks,
                        &actions,
                        &profile,
                        global_header,
                    );
                    *consumers.lock().unwrap_or_else(PoisonError::into_inner) = None;

                    if let Err(error) = result
                        && !mailbox.control.cancelled()
                    {
                        *publication.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some(Err(format!("{error:#}")));
                    }

                    mailbox.control.cancel();
                    mailbox.queue.discard_where(|_| true);
                }
            })?;

        Ok(Self {
            profile,
            muxer: route.muxer.clone(),
            actions,
            mailbox,
            consumers,
            publication,
            thread,
        })
    }

    pub(super) fn matches(&self, route: &Route, plan: &[Option<String>]) -> bool {
        self.muxer == route.muxer
            && self.profile == route.profile
            && self
                .actions
                .iter()
                .copied()
                .eq(plan.iter().map(Option::is_some))
    }

    pub(super) fn try_add(&self, consumer: &Arc<Mailbox>, route: &Arc<Route>) -> bool {
        let mut consumers = self
            .consumers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(consumers) = consumers.as_mut() else {
            return false;
        };

        consumers.retain(|consumer| consumer.mailbox.strong_count() != 0);
        consumers.push(Consumer {
            mailbox: Arc::downgrade(consumer),
            route: Arc::downgrade(route),
        });
        self.mailbox.control.notify_activity();

        true
    }

    pub(super) fn send(&self, packet: &Packet) {
        self.mailbox.send(packet);
    }

    pub(super) fn stop(self) -> JoinHandle<()> {
        self.mailbox.control.cancel();

        self.thread
    }
}

fn active_consumers(consumers: &[Consumer]) -> Vec<Arc<Mailbox>> {
    consumers
        .iter()
        .filter_map(|consumer| {
            let route = consumer.route.upgrade()?;

            (!route.stopped.load(Ordering::Acquire))
                .then(|| consumer.mailbox.upgrade())
                .flatten()
        })
        .filter(|mailbox| !mailbox.control.cancelled())
        .collect()
}

fn consumer_snapshot(consumers: &Consumers) -> Option<Vec<Arc<Mailbox>>> {
    let mut state = consumers.lock().unwrap_or_else(PoisonError::into_inner);
    let consumers = state.as_ref()?;
    let active = active_consumers(consumers);

    if !consumers.is_empty() && active.is_empty() {
        *state = None;

        return None;
    }

    Some(active)
}

fn run(
    mailbox: &Mailbox,
    consumers: &Consumers,
    publication: &TrackPublication,
    tracks: &[Track],
    actions: &[bool],
    profile: &Profile,
    global_header: bool,
) -> Result<()> {
    let mut encoders = Vec::new();
    let video = tracks.iter().any(|track| track.video);
    let mut waiting = video;
    let mut decoder_error_logged = false;
    let activity = mailbox.control.activity();

    while !mailbox.control.cancelled() {
        let observed = activity.snapshot();
        let Some(targets) = consumer_snapshot(consumers) else {
            break;
        };

        if mailbox.overflow.swap(false, Ordering::AcqRel) {
            for consumer in &targets {
                consumer.reset();
            }

            encoders.clear();
            waiting = video;
        }

        let Some(packet) = mailbox.queue.try_recv()? else {
            activity.wait(observed, Some(POLL));
            continue;
        };
        let Some((index, track)) = tracks
            .iter()
            .enumerate()
            .find(|(_, track)| track.input == packet.stream())
        else {
            continue;
        };

        if waiting && (!track.video || !packet.is_key()) {
            continue;
        }

        if encoders.is_empty() {
            let mut output_tracks = tracks.to_vec();

            for (index, track) in tracks.iter().enumerate() {
                let encoder = actions[index]
                    .then(|| TrackEncoder::new(track, profile, global_header, index))
                    .transpose()?;
                if let Some(encoder) = &encoder {
                    (
                        output_tracks[index].parameters,
                        output_tracks[index].time_base,
                    ) = encoder.parameters();
                }

                encoders.push(encoder);
            }

            *publication.lock().unwrap_or_else(PoisonError::into_inner) = Some(Ok(output_tracks));
            waiting = false;
        }

        if let Some(encoder) = &mut encoders[index] {
            let encoded = match encoder.submit(&packet) {
                Ok(encoded) => encoded,
                Err(error) if error.is::<DecodeFailure>() => {
                    if !decoder_error_logged {
                        log::warn!(channel = profile.channel_id;
                            "Live forwarding decoder restarting after error: {error:#}"
                        );
                        decoder_error_logged = true;
                    }

                    for consumer in &targets {
                        consumer.reset();
                    }

                    encoders.clear();
                    waiting = video;
                    continue;
                }
                Err(error) => return Err(error),
            };

            for (mut encoded, _) in encoded {
                encoded.set_stream(track.input);
                for target in &targets {
                    target.send(&encoded);
                }
            }
        } else {
            for target in &targets {
                target.send(&packet);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{OutputDestination, OutputTarget, Routes};
    use super::*;

    #[test]
    fn group_stops_accepting_consumers_when_the_last_one_is_gone() {
        let mut cfg = crate::OutputConfig::new(160, 90, 25, 48_000);

        cfg.forward_listener = Some(1);
        cfg.stream_type = crate::StreamType::Rtmp;

        let route = Routes::prepare(vec![OutputDestination {
            id: 1,
            config: cfg,
            target: OutputTarget::Stream {
                url: "rtmp://127.0.0.1/live/test".into(),
            },
        }])
        .unwrap()
        .pop()
        .unwrap();
        let route = Arc::new(route);
        let consumer = Mailbox::new();
        let group = Group::start(&route, Vec::new(), Vec::new()).unwrap();

        assert!(group.try_add(&consumer, &route));

        consumer.control.cancel();

        assert!(consumer_snapshot(&group.consumers).is_none());
        assert!(!group.try_add(&Mailbox::new(), &route));
        group.stop().join().unwrap();
    }
}
