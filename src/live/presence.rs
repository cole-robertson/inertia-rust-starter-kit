//! Presence: who is on a channel's stream right now (`channel.presence()`).
//!
//! A channel that returns `true` from [`super::Channel::tracks_presence`] accepts heartbeats:
//! each open page sends one every [`HEARTBEAT`] (`POST /live/presence`, with its tab id and an
//! optional `state` such as `{"editing": 7}`) and a `DELETE` when it closes; a tab not heard from
//! for [`EXPIRES_AFTER`] is gone. The room is the subscription's first stream key. When who is
//! there (or their state) changes, every subscriber of that key receives
//! `{identifier, type: "presence", present: [{user_id, name, state}]}`; `usePresence` in
//! `frontend/lib/live.ts` keeps the list. Heartbeats that change nothing send nothing.
//!
//! In memory, like the hub: one process (see the live-updates recipe for several).

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Once},
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;

use crate::models::users;

/// How often the browser says it is still there.
pub const HEARTBEAT: Duration = Duration::from_secs(15);
/// A tab not heard from for this long has left.
pub const EXPIRES_AFTER: Duration = Duration::from_secs(30);
/// The largest `state` (as JSON) a heartbeat may carry.
pub const MAX_STATE: usize = 1024;

#[derive(Debug, Clone)]
struct Tab {
    user_id: i64,
    name: String,
    state: Option<Value>,
    seen: Instant,
}

/// `(channel, key)` → tab id → tab.
type Rooms = HashMap<(String, String), HashMap<String, Tab>>;

static ROOMS: LazyLock<Mutex<Rooms>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SWEEPER: Once = Once::new();

fn rooms() -> std::sync::MutexGuard<'static, Rooms> {
    ROOMS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One person in a room. With several tabs, the first tab's state that isn't null wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Present {
    pub user_id: i64,
    pub name: String,
    pub state: Option<Value>,
}

fn people(room: Option<&HashMap<String, Tab>>) -> Vec<Present> {
    let mut by_user: HashMap<i64, Present> = HashMap::new();
    let mut tabs: Vec<(&String, &Tab)> = room.into_iter().flatten().collect();
    tabs.sort_by(|a, b| a.0.cmp(b.0));
    for (_, tab) in tabs {
        let entry = by_user.entry(tab.user_id).or_insert_with(|| Present {
            user_id: tab.user_id,
            name: tab.name.clone(),
            state: None,
        });
        if entry.state.is_none() {
            entry.state.clone_from(&tab.state);
        }
    }
    let mut out: Vec<Present> = by_user.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.user_id.cmp(&b.user_id)));
    out
}

/// Who is in `(channel, key)` now, by name.
#[must_use]
pub fn present(channel: &str, key: &str) -> Vec<Present> {
    people(rooms().get(&(channel.to_owned(), key.to_owned())))
}

fn announce(channel: &str, key: &str, present: Vec<Present>) {
    super::send(
        channel,
        key.to_owned(),
        super::Body::Presence(Arc::new(present)),
    );
}

/// A heartbeat from `tab` of `user` in `(channel, key)`, at `now`; who is there afterwards. Tells
/// the room when that changed.
pub fn beat(
    channel: &str,
    key: &str,
    tab: &str,
    user: &users::Model,
    state: Option<Value>,
) -> Vec<Present> {
    start_sweeper();
    beat_at(
        channel,
        key,
        tab,
        user.id,
        &user.name,
        state,
        Instant::now(),
    )
}

fn beat_at(
    channel: &str,
    key: &str,
    tab: &str,
    user_id: i64,
    name: &str,
    state: Option<Value>,
    now: Instant,
) -> Vec<Present> {
    let room_key = (channel.to_owned(), key.to_owned());
    let (before, after) = {
        let mut rooms = rooms();
        let before = people(rooms.get(&room_key));
        rooms.entry(room_key.clone()).or_default().insert(
            tab.to_owned(),
            Tab {
                user_id,
                name: name.to_owned(),
                state,
                seen: now,
            },
        );
        (before, people(rooms.get(&room_key)))
    };
    if before != after {
        announce(channel, key, after.clone());
    }
    after
}

/// `tab` of `user_id` left `(channel, key)` (only its own user may remove a tab).
pub fn leave(channel: &str, key: &str, tab: &str, user_id: i64) {
    let room_key = (channel.to_owned(), key.to_owned());
    let (before, after) = {
        let mut rooms = rooms();
        let before = people(rooms.get(&room_key));
        if let Some(room) = rooms.get_mut(&room_key) {
            if room.get(tab).is_some_and(|t| t.user_id == user_id) {
                room.remove(tab);
            }
            if room.is_empty() {
                rooms.remove(&room_key);
            }
        }
        (before, people(rooms.get(&room_key)))
    };
    if before != after {
        announce(channel, key, after);
    }
}

/// Drop tabs not heard from since `now - EXPIRES_AFTER`, telling each room that changed.
pub fn sweep(now: Instant) {
    let mut changed = Vec::new();
    {
        let mut rooms = rooms();
        rooms.retain(|key, room| {
            let before = people(Some(room));
            room.retain(|_, tab| now.duration_since(tab.seen) < EXPIRES_AFTER);
            let after = people(Some(room));
            if before != after {
                changed.push((key.clone(), after));
            }
            !room.is_empty()
        });
    }
    for ((channel, key), after) in changed {
        announce(&channel, &key, after);
    }
}

/// Expire stale tabs every few seconds. Started with the first heartbeat (a tokio runtime is
/// running by then).
fn start_sweeper() {
    SWEEPER.call_once(|| {
        tokio::spawn(async {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                sweep(Instant::now());
            }
        });
    });
}

/// Forget everyone (tests).
pub fn reset() {
    rooms().clear();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn arrivals_states_departures_and_expiry_are_announced_heartbeats_are_not() {
        let mut rx = super::super::HUB.subscribe();
        let (ch, key) = ("UnitPresenceChannel", "room-1");
        let t0 = Instant::now();
        let names = |p: &[Present]| p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();

        assert_eq!(names(&beat_at(ch, key, "a", 1, "One", None, t0)), ["One"]);
        // The same tab again, and a second tab of the same person: nothing new.
        beat_at(ch, key, "a", 1, "One", None, t0 + HEARTBEAT);
        beat_at(ch, key, "a2", 1, "One", None, t0 + HEARTBEAT);
        let editing = Some(json!({ "editing": 7 }));
        let now = beat_at(ch, key, "b", 2, "Two", editing.clone(), t0 + HEARTBEAT);
        assert_eq!(now[1].state, editing);
        // Someone else can't remove your tab.
        leave(ch, key, "b", 1);
        assert_eq!(present(ch, key).len(), 2);
        // Only "b" keeps beating; "a" and "a2" expire.
        beat_at(ch, key, "b", 2, "Two", None, t0 + HEARTBEAT * 2);
        beat_at(ch, key, "b", 2, "Two", None, t0 + HEARTBEAT * 3);
        sweep(t0 + HEARTBEAT * 3);
        assert_eq!(names(&present(ch, key)), ["Two"]);
        leave(ch, key, "b", 2);
        assert!(present(ch, key).is_empty());

        let mut announced = Vec::new();
        while let Ok(b) = rx.try_recv() {
            if let (true, super::super::Body::Presence(p)) = (b.channel == ch, &b.body) {
                announced.push(names(p));
            }
        }
        assert_eq!(
            announced,
            vec![
                vec!["One".to_owned()],
                vec!["One".to_owned(), "Two".to_owned()],
                vec!["One".to_owned(), "Two".to_owned()], // Two stopped editing
                vec!["Two".to_owned()],                   // One expired
                vec![],                                   // Two left
            ]
        );
    }
}
