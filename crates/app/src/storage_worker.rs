//! A failed history database must not disable configuration or status export.
use super::runtime::{Event, Events, Storage};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use hb_core::*;
use hb_storage::Store;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
pub(super) fn run(
    folder: PathBuf,
    messages: Receiver<Storage>,
    boot: Sender<Estimator>,
    events: Events,
) {
    let mut database = Store::open(&folder.join("history.db"));
    let _ = boot.send(
        database
            .as_ref()
            .map_or_else(|_| Estimator::default(), Store::load_estimator),
    );
    let clock = SystemClock::default();
    if let Ok(db) = &mut database {
        let _ = db.prune(clock.unix());
    } else if let Err(e) = &database {
        let _ = events.send(Event::Error(format!("History: {e}")));
    }
    let mut flush = Instant::now();
    let mut estimator = None;
    let mut pending_usage: Vec<UsageObservation> = Vec::new();
    loop {
        let message =
            messages.recv_timeout(Duration::from_secs(60).saturating_sub(flush.elapsed()));
        let result = match message {
            Ok(Storage::UsageSample(observations)) => {
                if let Ok(db) = &mut database {
                    observations.iter().try_for_each(|o| db.record_usage(o))
                } else {
                    for observation in observations {
                        if let Some(old) = pending_usage
                            .iter_mut()
                            .find(|old| old.reading.key == observation.reading.key)
                        {
                            *old = observation;
                        } else if pending_usage.len() < 512 {
                            pending_usage.push(observation);
                        }
                    }
                    Ok(())
                }
            }
            Ok(Storage::Insights(key, until, id)) => {
                let result = match &mut database {
                    Ok(db) => db.flush().and_then(|_| db.query_insights(&key, until)),
                    Err(e) => Err(e.clone()),
                };
                let _ = events.send(Event::Insights(id, result));
                Ok(())
            }
            Ok(Storage::Save(settings)) => {
                hb_storage::save_settings(&folder.join("config.json"), &settings)
            }
            Ok(Storage::Status(snapshot, running)) => {
                hb_storage::write_status(&folder.join("status.json"), &snapshot, running)
            }
            Ok(Storage::RemoveStatus) => match std::fs::remove_file(folder.join("status.json")) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            },
            Ok(Storage::State(state)) => {
                if let Ok(db) = &mut database {
                    db.save_estimator(&state)
                } else {
                    estimator = Some(state);
                    Ok(())
                }
            }
            Ok(Storage::History(key, since, until, width, id)) => {
                let result = match &mut database {
                    Ok(db) => db
                        .flush()
                        .and_then(|_| db.query_with_baseline(&key, since, until, width))
                        .map(|readings| HistorySeries::calendar(readings, since, until)),
                    Err(e) => Err(e.clone()),
                };
                let _ = events.send(Event::History(id, result));
                Ok(())
            }
            Ok(Storage::UsageHistory(key, seconds, until, width, id)) => {
                let result = match &mut database {
                    Ok(db) => db
                        .flush()
                        .and_then(|_| db.query_usage(&key, until, seconds, width)),
                    Err(e) => Err(e.clone()),
                };
                let _ = events.send(Event::History(id, result));
                Ok(())
            }
            Ok(Storage::Quit) | Err(RecvTimeoutError::Disconnected) => {
                if let Ok(db) = &mut database
                    && let Err(e) = db.flush()
                {
                    let _ = events.send(Event::Error(format!("History flush: {e}")));
                }
                break;
            }
            Err(RecvTimeoutError::Timeout) => Ok(()),
        };
        if let Err(e) = result {
            let _ = events.send(Event::Error(format!("Storage: {e}")));
        }
        if flush.elapsed() >= Duration::from_secs(60) {
            if database.is_err() {
                database = Store::open(&folder.join("history.db"));
                if let Ok(db) = &mut database {
                    for observation in pending_usage.drain(..) {
                        let _ = db.record_usage(&observation);
                    }
                    if let Some(state) = estimator.take() {
                        let _ = db.save_estimator(&state);
                    }
                }
            }
            if let Ok(db) = &mut database
                && let Err(e) = db.flush().and_then(|_| db.prune(clock.unix()))
            {
                let _ = events.send(Event::Error(format!("History: {e}")));
            }
            flush = Instant::now();
        }
    }
}
