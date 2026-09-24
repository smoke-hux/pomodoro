use super::*;
use crate::sound::{play, play_and_report};
use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

#[test]
fn the_alarm_waits_behind_a_lesser_sound_and_everything_else_mid_sound_is_dropped() {
    use Admission::{Drop, Start, Wait};
    use Cue::{IntervalFinished as Alarm, TaskDone as Chime};

    assert_eq!(admit(None, false, Alarm), Start);
    assert_eq!(admit(None, false, Chime), Start);
    // Ticking a task off a second before the interval ends used to cost
    // the alarm; with notifications off the interval then ended unmarked.
    assert_eq!(admit(Some(Chime), false, Alarm), Wait);
    assert_eq!(
        admit(Some(Chime), true, Alarm),
        Drop,
        "one alarm waiting is enough"
    );
    assert_eq!(admit(Some(Alarm), false, Alarm), Drop);
    assert_eq!(admit(Some(Alarm), false, Chime), Drop);
    assert_eq!(admit(Some(Chime), false, Chime), Drop);
}

/// What the stand-in `play` functions below were asked to play.
static PLAYED: Mutex<Vec<Cue>> = Mutex::new(Vec::new());

#[test]
fn an_alarm_asked_for_mid_chime_is_played_next_and_its_asker_is_told() {
    static SLOT: Mutex<Slot> = Mutex::new(Slot {
        playing: None,
        waiting: None,
    });
    fn slow(cue: Cue) -> Result<(), String> {
        PLAYED.lock().unwrap().push(cue);
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    request(&SLOT, Cue::TaskDone, None, slow);
    let (reply, outcome) = mpsc::channel();
    request(&SLOT, Cue::IntervalFinished, Some(reply), slow);
    // A chime arriving now is dropped, and says so at once.
    let (dropped, refusal) = mpsc::channel();
    request(&SLOT, Cue::TaskDone, Some(dropped), slow);
    assert!(refusal
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());

    assert_eq!(
        outcome.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(())
    );
    assert_eq!(
        *PLAYED.lock().unwrap(),
        [Cue::TaskDone, Cue::IntervalFinished]
    );

    // And the slot is free again afterwards.
    for _ in 0..50 {
        if SLOT.lock().unwrap().playing.is_none() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(SLOT.lock().unwrap().playing.is_none());
}

#[test]
fn a_sound_that_fails_is_reported_to_whoever_asked() {
    static SLOT: Mutex<Slot> = Mutex::new(Slot {
        playing: None,
        waiting: None,
    });
    fn broken(_: Cue) -> Result<(), String> {
        Err("no player".to_string())
    }
    let (reply, outcome) = mpsc::channel();
    request(&SLOT, Cue::IntervalFinished, Some(reply), broken);
    assert_eq!(
        outcome.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err("no player".to_string())
    );
}

#[test]
fn the_slot_is_given_back_when_the_playing_thread_panics_or_never_starts() {
    static SLOT: Mutex<Slot> = Mutex::new(Slot {
        playing: None,
        waiting: None,
    });
    let (reply, outcome) = mpsc::channel();
    {
        let mut slot = SLOT.lock().unwrap();
        slot.playing = Some(Cue::TaskDone);
        slot.waiting = Some(Request {
            cue: Cue::IntervalFinished,
            reply: Some(reply),
        });
    }
    let occupied = Occupied(&SLOT);
    let unwound = thread::spawn(move || {
        let _occupied = occupied;
        panic!("a panic on the playing thread, on purpose");
    })
    .join();
    assert!(unwound.is_err());

    let slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        slot.playing.is_none(),
        "the slot was left marked as playing"
    );
    assert!(slot.waiting.is_none());
    assert!(
        outcome.recv().unwrap().is_err(),
        "whoever was waiting is told"
    );
}

#[test]
fn a_released_slot_is_not_emptied_again_behind_the_next_sound() {
    static SLOT: Mutex<Slot> = Mutex::new(Slot {
        playing: None,
        waiting: None,
    });
    let occupied = Occupied(&SLOT);
    // The ordinary route has emptied the slot and unlocked it, and a new
    // sound, with an alarm waiting behind it, has already been admitted.
    let (reply, outcome) = mpsc::channel();
    {
        let mut slot = SLOT.lock().unwrap();
        slot.playing = Some(Cue::TaskDone);
        slot.waiting = Some(Request {
            cue: Cue::IntervalFinished,
            reply: Some(reply),
        });
    }
    occupied.release();

    let slot = SLOT.lock().unwrap();
    assert_eq!(slot.playing, Some(Cue::TaskDone));
    assert_eq!(
        slot.waiting.as_ref().map(|next| next.cue),
        Some(Cue::IntervalFinished)
    );
    assert!(outcome.try_recv().is_err(), "the waiting alarm was dropped");
}

#[test]
fn sounds_asked_for_from_many_threads_never_play_over_one_another() {
    static SLOT: Mutex<Slot> = Mutex::new(Slot {
        playing: None,
        waiting: None,
    });
    static AT_ONCE: AtomicU32 = AtomicU32::new(0);
    static MOST: AtomicU32 = AtomicU32::new(0);
    fn counted(_: Cue) -> Result<(), String> {
        let now = AT_ONCE.fetch_add(1, Ordering::SeqCst) + 1;
        MOST.fetch_max(now, Ordering::SeqCst);
        AT_ONCE.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
    let askers: Vec<_> = (0..4)
        .map(|_| {
            thread::spawn(|| {
                for _ in 0..20_000 {
                    request(&SLOT, Cue::TaskDone, None, counted);
                }
            })
        })
        .collect();
    for asker in askers {
        asker.join().unwrap();
    }
    for _ in 0..100 {
        if SLOT.lock().unwrap().playing.is_none() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(MOST.load(Ordering::SeqCst), 1, "two sounds played at once");
}

#[test]
fn playing_does_nothing_under_test() {
    play(Cue::IntervalFinished);
    play(Cue::TaskDone);
    assert_eq!(play_and_report(Cue::IntervalFinished), Ok(()));
    assert!(SLOT.lock().unwrap().playing.is_none());
}
