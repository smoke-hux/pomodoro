//! Serial playback and priority admission: completed intervals may wait behind a chime.

use super::{play_now, Cue};
use std::{
    sync::{mpsc, Mutex, PoisonError},
    thread,
};

/// Somebody waiting to hear how their request went.
type Reply = mpsc::Sender<Result<(), String>>;

/// A sound asked for, and who to tell.
struct Request {
    cue: Cue,
    reply: Option<Reply>,
}

/// What is playing, and the one request allowed to wait behind it.
#[derive(Default)]
struct Slot {
    playing: Option<Cue>,
    waiting: Option<Request>,
}

/// What becomes of a new request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// Nothing is playing: play it.
    Start,
    /// It waits for the sound now playing and goes next.
    Wait,
    /// It is dropped.
    Drop,
}

/// Decides what becomes of a request for `cue`.
///
/// The end of an interval waits behind a lesser sound rather than being lost
/// to it. Everything else that arrives mid-sound is dropped — a second chime on
/// top of the first helps nobody, and an alarm already playing or already
/// waiting does not need another behind it.
fn admit(playing: Option<Cue>, something_waiting: bool, cue: Cue) -> Admission {
    match (playing, cue) {
        (None, _) => Admission::Start,
        (Some(Cue::TaskDone), Cue::IntervalFinished) if !something_waiting => Admission::Wait,
        _ => Admission::Drop,
    }
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    playing: None,
    waiting: None,
});

/// Empties the slot when the playing thread ends by any route other than the
/// ordinary one, which has already emptied it: a panic's unwinding, or a thread
/// that could not be started and whose closure is thrown away. A slot left
/// marked as playing would silence Pomodoro until it was restarted.
///
/// The ordinary route must [`release`](Occupied::release) it. The slot is free
/// the moment that route unlocks it, and a new sound may be admitted before
/// this thread is gone: emptying the slot again then would mark that sound as
/// not playing, letting a third play over it, and throw away an alarm waiting
/// behind it.
struct Occupied(&'static Mutex<Slot>);

impl Occupied {
    /// Lets go without touching the slot, for the route that has emptied it.
    fn release(self) {
        std::mem::forget(self);
    }
}

impl Drop for Occupied {
    fn drop(&mut self) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.playing = None;
        if let Some(Request {
            reply: Some(reply), ..
        }) = slot.waiting.take()
        {
            let _ = reply.send(Err("The sound could not be played.".to_string()));
        }
    }
}

fn request(
    slot: &'static Mutex<Slot>,
    cue: Cue,
    reply: Option<Reply>,
    play: fn(Cue) -> Result<(), String>,
) {
    let refused = |reply: Option<Reply>| {
        if let Some(reply) = reply {
            let _ = reply.send(Err(
                "Another sound is playing. Try again in a moment.".to_string()
            ));
        }
    };
    {
        let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
        match admit(held.playing, held.waiting.is_some(), cue) {
            Admission::Start => held.playing = Some(cue),
            Admission::Wait => {
                held.waiting = Some(Request { cue, reply });
                return;
            }
            Admission::Drop => {
                drop(held);
                refused(reply);
                return;
            }
        }
    }

    let occupied = Occupied(slot);
    let spawned = thread::Builder::new()
        .name("sound-player".to_string())
        .spawn(move || {
            let occupied = occupied;
            let mut current = Request { cue, reply };
            loop {
                let result = play(current.cue);
                if let Err(error) = &result {
                    eprintln!("could not play a sound: {error}");
                }
                if let Some(reply) = current.reply.take() {
                    let _ = reply.send(result);
                }
                let mut held = occupied.0.lock().unwrap_or_else(PoisonError::into_inner);
                match held.waiting.take() {
                    Some(next) => {
                        held.playing = Some(next.cue);
                        current = next;
                    }
                    None => {
                        held.playing = None;
                        drop(held);
                        occupied.release();
                        break;
                    }
                }
            }
        });
    if let Err(error) = spawned {
        // The closure, and the `Occupied` inside it, has been dropped: the slot
        // is free again and anything that was waiting has been told.
        eprintln!("could not play a sound: {error}");
    }
}

pub(super) fn enqueue(cue: Cue, reply: Option<Reply>) {
    request(&SLOT, cue, reply, play_now);
}

#[cfg(test)]
mod tests;
