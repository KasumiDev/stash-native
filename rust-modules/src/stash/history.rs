use super::{Client, Error};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum HistoryCommand {
    SaveActivity {
        id: String,
        resume_time: f64,
        watched_delta: f64,
    },
    AddPlay {
        id: String,
    },
    AddO {
        id: String,
    },
}

/// Playback clock accounting. Caller supplies elapsed wall time only while actual frames play;
/// seek, pause, buffering, and preview time must have `playing == false`.
pub struct HistoryTracker {
    id: String,
    watched: Duration,
    pending: Duration,
    since_save: Duration,
    position: f64,
    play_sent: bool,
    o_in_flight: bool,
    o_key_down: bool,
}

impl HistoryTracker {
    pub fn new(id: String) -> Self {
        Self {
            id,
            watched: Duration::ZERO,
            pending: Duration::ZERO,
            since_save: Duration::ZERO,
            position: 0.0,
            play_sent: false,
            o_in_flight: false,
            o_key_down: false,
        }
    }

    pub fn tick(&mut self, elapsed: Duration, position: f64, playing: bool) -> Vec<HistoryCommand> {
        if position.is_finite() && position >= 0.0 {
            self.position = position;
        }
        if !playing {
            return Vec::new();
        }
        self.watched = self.watched.saturating_add(elapsed);
        self.pending = self.pending.saturating_add(elapsed);
        self.since_save = self.since_save.saturating_add(elapsed);
        let mut commands = Vec::new();
        if !self.play_sent && self.watched >= Duration::from_secs(30) {
            self.play_sent = true;
            commands.push(HistoryCommand::AddPlay {
                id: self.id.clone(),
            });
        }
        if self.since_save >= Duration::from_secs(15) {
            commands.push(self.flush(false));
        }
        commands
    }

    /// Drain deltas before dispatch, including failures: ambiguous requests must not be replayed.
    pub fn flush(&mut self, completed: bool) -> HistoryCommand {
        let delta = std::mem::replace(&mut self.pending, Duration::ZERO);
        self.since_save = Duration::ZERO;
        HistoryCommand::SaveActivity {
            id: self.id.clone(),
            resume_time: if completed { 0.0 } else { self.position },
            watched_delta: delta.as_secs_f64(),
        }
    }

    /// Feed both down and up edges. Repeated down events cannot increment the counter again.
    pub fn o_key(&mut self, down: bool) -> Option<HistoryCommand> {
        let rising = down && !self.o_key_down;
        self.o_key_down = down;
        if !rising || self.o_in_flight {
            return None;
        }
        self.o_in_flight = true;
        Some(HistoryCommand::AddO {
            id: self.id.clone(),
        })
    }
    pub fn o_finished(&mut self) {
        self.o_in_flight = false;
    }
    pub fn watched(&self) -> Duration {
        self.watched
    }
}

/// A bounded FIFO shared by clones. A single worker calls run_next; network I/O stays off UI.
/// The execution lock keeps multiple accidental workers serialized. Dequeued requests are never
/// reinserted on error, because additive activity mutations are not idempotent.
#[derive(Clone, Default)]
pub struct MutationQueue {
    queue: Arc<Mutex<VecDeque<HistoryCommand>>>,
    execution: Arc<Mutex<()>>,
}

impl MutationQueue {
    pub fn push(&self, command: HistoryCommand) -> Result<(), Error> {
        let mut queue = self.queue.lock().map_err(|_| Error::Storage)?;
        if queue.len() >= 64 {
            return Err(Error::Storage);
        }
        queue.push_back(command);
        Ok(())
    }

    pub fn run_next(
        &self,
        client: &Client,
    ) -> Option<(HistoryCommand, Result<Option<i64>, Error>)> {
        let _serial = self.execution.lock().ok()?;
        let command = self.queue.lock().ok()?.pop_front()?;
        let result = match &command {
            HistoryCommand::SaveActivity {
                id,
                resume_time,
                watched_delta,
            } => client
                .save_activity(id, *resume_time, *watched_delta)
                .map(|_| None),
            HistoryCommand::AddPlay { id } => client.add_play(id).map(|_| None),
            HistoryCommand::AddO { id } => client.add_o(id).map(Some),
        };
        Some((command, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignores_buffer_pause_seek_and_reports_deltas_once() {
        let mut h = HistoryTracker::new("synthetic-scene".into());
        assert!(h.tick(Duration::from_secs(90), 500.0, false).is_empty());
        let commands = h.tick(Duration::from_secs(15), 515.0, true);
        assert_eq!(
            commands,
            vec![HistoryCommand::SaveActivity {
                id: "synthetic-scene".into(),
                resume_time: 515.0,
                watched_delta: 15.0
            }]
        );
        let commands = h.tick(Duration::from_secs(15), 530.0, true);
        assert!(matches!(commands[0], HistoryCommand::AddPlay { .. }));
        assert_eq!(h.tick(Duration::from_secs(30), 560.0, true).len(), 1);
        assert_eq!(
            h.flush(true),
            HistoryCommand::SaveActivity {
                id: "synthetic-scene".into(),
                resume_time: 0.0,
                watched_delta: 0.0
            }
        );
    }
    #[test]
    fn explicit_o_count_suppresses_repeat_and_overlap() {
        let mut h = HistoryTracker::new("synthetic-scene".into());
        assert!(h.o_key(true).is_some());
        assert!(h.o_key(true).is_none());
        h.o_key(false);
        assert!(h.o_key(true).is_none());
        h.o_finished();
        assert!(h.o_key(true).is_none());
        h.o_key(false);
        assert!(h.o_key(true).is_some());
    }
}
