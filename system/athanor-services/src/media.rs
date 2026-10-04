//! MPRIS players, as the media controls show them (doc_bar.md, BR3; doc_control_center.md,
//! CC3 and CC8). The model finds the players on the session bus (`ListNames`, then
//! `NameOwnerChanged` for names under `org.mpris.MediaPlayer2.`), follows each through a
//! mirror of its own, and publishes a [`MediaState`]. A mirror follows its player's unique
//! owner, so a signal from any other sender is ignored.
//!
//! Everything a player sends is untrusted: titles are sanitised, a property of the wrong type
//! reads as absent, and artwork is a regular file on disk or a small `data:` image, never a
//! remote one.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::future::Future;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use athanor_unit::text::{line, NAME_CHARS};
use futures_util::future::select_all;
use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use zbus::fdo::DBusProxy;
use zbus::proxy::{Builder, CacheProperties, MethodFlags};
use zbus::Connection;

use crate::mirror::{self, Snapshot, Source, TIMEOUT};
use crate::props::{self, Props};
use crate::runtime::{Bus, Buses};

pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
pub const MPRIS_ROOT: &str = "org.mpris.MediaPlayer2";
pub const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// Players followed at once. A process on the session bus can own any number of names under
/// the prefix, and each followed name costs a mirror: the first ones to appear are kept.
pub const MAX_PLAYERS: usize = 16;
/// The largest artwork a `data:` URI may carry once decoded. Larger images are dropped; a
/// player that wants a cover shown names a file.
pub const MAX_ART_BYTES: usize = 1 << 20;
/// The longest path a `file:` URI may name, as `PATH_MAX`.
const MAX_PATH_BYTES: usize = 4096;
/// How long the model waits for the file system to open and read an artwork file.
const STAT_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: Option<String>,
}

/// A player's cover: the image's bytes, at most [`MAX_ART_BYTES`], read by the model from a
/// `file:` URI's regular file or decoded from a `data:` URI. Shared, so a state is cloned and
/// compared without copying up to 1 MiB per player.
#[derive(Clone, PartialEq, Eq)]
pub enum Art {
    Data(Arc<[u8]>),
}

impl std::fmt::Debug for Art {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Art::Data(bytes) => write!(f, "Data({} bytes)", bytes.len()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub bus_name: String,
    pub identity: String,
    pub track: Option<Track>,
    pub playing: bool,
    /// Beyond the plan's fields: the bar's skip buttons are sensitive only when the player
    /// can skip.
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    /// As of the last time the player's properties were read: a player does not signal its
    /// position as it advances, so a reader that draws a seek bar extrapolates from it.
    pub position_us: Option<i64>,
    pub length_us: Option<i64>,
    pub art: Option<Art>,
}

/// What the media controls show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaState {
    /// Sorted by bus name.
    pub players: Vec<Player>,
    /// The player the controls act on: the one the person chose, else the first by name.
    pub current: Option<usize>,
    /// Counts the commands a player refused or did not answer: the state is published again
    /// after each, so a reader that sees this change knows its last action did not complete.
    /// Beyond the plan's fields, so the bar keeps its "action did not complete" note.
    pub refused: u32,
    /// The session bus's names were listed, and the current player, if any, has answered with
    /// its properties: the controls show what they will keep showing. Beyond the plan's
    /// fields, for the bar's `settled`.
    pub settled: bool,
}

impl MediaState {
    pub fn current_player(&self) -> Option<&Player> {
        self.players.get(self.current?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    /// MPRIS `Seek`: a signed offset from the current position, in microseconds.
    Seek(i64),
    /// Makes the player of this bus name current while it is on the bus; ignored for a name
    /// that is not.
    Choose(String),
}

/// A well-known name of an MPRIS player.
pub fn is_player(name: &str) -> bool {
    name.strip_prefix(MPRIS_PREFIX)
        .is_some_and(|rest| !rest.is_empty())
}

/// The metadata dictionary, `a{sv}`; `None` for any other type.
fn metadata(player: &Props) -> Option<Props> {
    let value = player.get("Metadata")?.try_clone().ok()?;
    let entries: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
        value.try_into().ok()?;
    Some(entries.into_iter().collect())
}

/// The track from a player's `Metadata`; `None` without a title.
pub fn track(player: &Props) -> Option<Track> {
    let metadata = metadata(player)?;
    let title = props::get_str(&metadata, "xesam:title")
        .map(|title| line(title, NAME_CHARS))
        .filter(|title| !title.trim().is_empty())?;
    let artist = props::get_strs(&metadata, "xesam:artist")
        .map(|artists| line(&artists.join(", "), NAME_CHARS))
        .filter(|artist| !artist.trim().is_empty());
    Some(Track { title, artist })
}

pub fn playing(player: &Props) -> bool {
    props::get_str(player, "PlaybackStatus") == Some("Playing")
}

fn length_us(player: &Props) -> Option<i64> {
    let metadata = metadata(player)?;
    props::get_i64(&metadata, "mpris:length").or_else(|| {
        props::get_u64(&metadata, "mpris:length").and_then(|length| i64::try_from(length).ok())
    })
}

/// A player from its two interfaces' properties and its checked cover.
fn read_player(bus_name: &str, player: &Props, root: &Props, art: Option<Art>) -> Player {
    let fallback = bus_name.strip_prefix(MPRIS_PREFIX).unwrap_or(bus_name);
    let identity = props::get_str(root, "Identity")
        .map(|identity| line(identity, NAME_CHARS))
        .filter(|identity| !identity.trim().is_empty())
        .unwrap_or_else(|| line(fallback, NAME_CHARS));
    let flag = |name| props::get_bool(player, name).unwrap_or(false);
    Player {
        bus_name: bus_name.to_owned(),
        identity,
        track: track(player),
        playing: playing(player),
        can_next: flag("CanGoNext"),
        can_previous: flag("CanGoPrevious"),
        can_seek: flag("CanSeek"),
        position_us: props::get_i64(player, "Position"),
        length_us: length_us(player),
        art,
    }
}

/// The path a `file:` URI names: no host but `localhost`, percent-decoded, absolute, with no
/// query or fragment and no NUL.
fn file_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return None;
    }
    let mut bytes = Vec::with_capacity(path.len());
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let hex = |digit: Option<u8>| digit.and_then(|d| char::from(d).to_digit(16));
        let (high, low) = (hex(input.next())?, hex(input.next())?);
        bytes.push(u8::try_from(high * 16 + low).ok()?);
    }
    if bytes.contains(&0) || bytes.len() > MAX_PATH_BYTES {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// Standard base64, padding optional; `None` for a character outside the alphabet.
fn base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
    let (mut accumulated, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let sextet = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        accumulated = accumulated << 6 | u32::from(sextet);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulated >> bits) as u8);
            accumulated &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The bytes of `data:image/...;base64,...` within [`MAX_ART_BYTES`].
fn data_uri(rest: &str) -> Option<Vec<u8>> {
    let (header, payload) = rest.split_once(',')?;
    let media = header.strip_suffix(";base64")?;
    if !media.starts_with("image/") || payload.len() > MAX_ART_BYTES / 3 * 4 + 4 {
        return None;
    }
    base64(payload).filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_ART_BYTES)
}

/// The bytes of the regular file at `path`, at most [`MAX_ART_BYTES`]. The file is opened
/// once without blocking and judged by `fstat` of the open descriptor, so a path swapped for a
/// FIFO or a device after any earlier look cannot hang the model, and a symlink to one is
/// refused too. At most one byte more than the limit is read, so a file that grows is
/// refused instead of read whole.
fn read_art_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || usize::try_from(metadata.size()).ok()? > MAX_ART_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_ART_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_ART_BYTES).then_some(bytes)
}

/// The cover a player's `mpris:artUrl` names (CC8). `file:` is read here, by a blocking task
/// bounded by [`STAT_TIMEOUT`]; `https:` and anything else is `None`, since the panel may open
/// no network connection.
async fn resolve_art(url: &str) -> Option<Art> {
    if let Some(rest) = url.strip_prefix("data:") {
        return data_uri(rest).map(|bytes| Art::Data(bytes.into()));
    }
    let path = file_path(url)?;
    let read = tokio::task::spawn_blocking(move || read_art_file(&path));
    let bytes = timeout(STAT_TIMEOUT, read).await.ok()?.ok()??;
    Some(Art::Data(bytes.into()))
}

/// Starts the model on `handle`'s runtime. The receiver ends when the model does: without the
/// session bus it publishes a settled, empty state and ends.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
) -> (
    watch::Receiver<MediaState>,
    mpsc::UnboundedSender<MediaCommand>,
) {
    spawn_with(handle, buses, TIMEOUT)
}

/// [`spawn`] with the time a player's reply may take.
fn spawn_with(
    handle: &Handle,
    buses: Buses,
    call_timeout: Duration,
) -> (
    watch::Receiver<MediaState>,
    mpsc::UnboundedSender<MediaCommand>,
) {
    let (state, rx) = watch::channel(MediaState::default());
    let (commands, command_rx) = mpsc::unbounded_channel();
    let model = handle.clone();
    handle.spawn(async move {
        let connection = match buses.connection(Bus::Session).await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!(error = %err, "no session bus; the media controls are hidden");
                state.send_modify(|state| state.settled = true);
                return;
            }
        };
        run(&model, connection, state, command_rx, call_timeout).await;
    });
    (rx, commands)
}

/// A player the model follows.
struct Followed {
    mirror: watch::Receiver<Snapshot>,
    /// The cover of the last `mpris:artUrl` seen, so the file system is asked once per URL.
    art: Option<(String, Option<Art>)>,
}

/// Resolves when a followed player's mirror changes or ends; `false` for a mirror that ended.
async fn any_changed(followed: &mut BTreeMap<String, Followed>) -> (String, bool) {
    if followed.is_empty() {
        return std::future::pending().await;
    }
    let waits = followed.iter_mut().map(|(name, player)| {
        Box::pin(async move { (name.clone(), player.mirror.changed().await.is_ok()) })
    });
    select_all(waits).await.0
}

fn follow(
    handle: &Handle,
    connection: &Connection,
    followed: &mut BTreeMap<String, Followed>,
    name: &str,
) -> bool {
    if followed.contains_key(name) {
        return false;
    }
    if followed.len() >= MAX_PLAYERS {
        tracing::warn!(name, "too many media players; this one is not followed");
        return true;
    }
    let mirror = mirror::spawn(
        handle,
        connection.clone(),
        name,
        Source::Fixed(vec![(MPRIS_PATH, MPRIS_ROOT), (MPRIS_PATH, MPRIS_PLAYER)]),
    );
    followed.insert(name.to_owned(), Followed { mirror, art: None });
    false
}

/// Follows the players `ListNames` shows, as many as the cap allows. Returns whether one was
/// turned away at the cap, so the model lists again when a followed player leaves.
async fn follow_listed(
    dbus: &DBusProxy<'_>,
    handle: &Handle,
    connection: &Connection,
    followed: &mut BTreeMap<String, Followed>,
) -> bool {
    let mut turned_away = false;
    match timeout(TIMEOUT, dbus.list_names()).await {
        Ok(Ok(names)) => {
            for name in names.iter().filter(|name| is_player(name)) {
                turned_away |= follow(handle, connection, followed, name);
            }
        }
        // Listed all the same: a player that appears later is followed, and the controls
        // need not wait for a listing that is not coming.
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "ListNames failed; players are followed as they appear")
        }
        Err(_) => tracing::warn!("ListNames did not answer; players are followed as they appear"),
    }
    turned_away
}

async fn run(
    handle: &Handle,
    connection: Connection,
    state: watch::Sender<MediaState>,
    mut commands: mpsc::UnboundedReceiver<MediaCommand>,
    call_timeout: Duration,
) {
    let dbus = match DBusProxy::new(&connection).await {
        Ok(dbus) => dbus,
        Err(err) => {
            tracing::warn!(error = %err, "no bus proxy; the media controls are hidden");
            state.send_modify(|state| state.settled = true);
            return;
        }
    };
    // Subscribe before listing, so no player falls between the two.
    let mut owners = match dbus.receive_name_owner_changed().await {
        Ok(owners) => owners,
        Err(err) => {
            tracing::warn!(error = %err, "no name owner changes; the media controls are hidden");
            state.send_modify(|state| state.settled = true);
            return;
        }
    };
    let mut followed: BTreeMap<String, Followed> = BTreeMap::new();
    let mut turned_away = follow_listed(&dbus, handle, &connection, &mut followed).await;
    let mut chosen: Option<String> = None;
    let mut refused = 0;
    let mut last = read_state(&mut followed, &mut chosen, refused).await;
    state.send_replace(last.clone());
    // Commands wait here while one runs. A new seek replaces a waiting one, so a drag of a
    // seek bar ends on its last position and the player is not asked for every step.
    let mut waiting: VecDeque<(String, MediaCommand)> = VecDeque::new();
    let mut running: Option<Running> = None;
    loop {
        let mut relevant = true;
        let mut freed = false;
        tokio::select! {
            change = owners.next() => {
                let Some(change) = change else { return };
                match change.args() {
                    Ok(args) if is_player(args.name().as_str()) => {
                        if args.new_owner().is_some() {
                            turned_away |= follow(handle, &connection, &mut followed, args.name().as_str());
                        } else {
                            // Dropping the mirror's receiver ends it.
                            freed = followed.remove(args.name().as_str()).is_some();
                        }
                    }
                    _ => relevant = false,
                }
            }
            (name, alive) = any_changed(&mut followed) => {
                if !alive {
                    freed = followed.remove(&name).is_some();
                }
            }
            command = commands.recv() => match command {
                None => return,
                Some(MediaCommand::Choose(name)) => {
                    if followed.contains_key(&name) {
                        chosen = Some(name);
                    }
                }
                Some(command) => {
                    // The player the person saw when asking, not the one current when the
                    // queue reaches the command.
                    let target = last.current_player().map(|player| player.bus_name.clone());
                    if let Some(target) = target {
                        if matches!(command, MediaCommand::Seek(_)) {
                            waiting.retain(|(_, queued)| !matches!(queued, MediaCommand::Seek(_)));
                        }
                        waiting.push_back((target, command));
                    }
                }
            },
            result = async {
                match running.as_mut() {
                    Some(call) => call.await,
                    None => std::future::pending().await,
                }
            }, if running.is_some() => {
                running = None;
                if let Err(err) = result {
                    tracing::warn!(error = %err, "a media player refused or did not answer");
                    refused += 1;
                }
            }
        }
        // A name turned away at the cap is followed once a slot is free.
        if freed && turned_away {
            turned_away = follow_listed(&dbus, handle, &connection, &mut followed).await;
        }
        if running.is_none() {
            running = waiting.pop_front().map(|(player, command)| {
                Box::pin(execute(connection.clone(), player, command, call_timeout)) as Running
            });
        }
        if !relevant {
            continue;
        }
        let next = read_state(&mut followed, &mut chosen, refused).await;
        last = next.clone();
        state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
    }
}

/// The state from the mirrors. A player whose owner is gone is not listed.
async fn read_state(
    followed: &mut BTreeMap<String, Followed>,
    chosen: &mut Option<String>,
    refused: u32,
) -> MediaState {
    let mut players = Vec::new();
    let mut settled = true;
    for (name, player) in followed.iter_mut() {
        let url = {
            let snapshot = player.mirror.borrow();
            if snapshot.owner.is_none() {
                // Owner not looked up yet, or gone and about to be removed.
                settled &= snapshot.generation > 0;
                continue;
            }
            props::lookup(&snapshot.objects, MPRIS_PATH, MPRIS_PLAYER)
                .and_then(metadata)
                .and_then(|metadata| props::get_str(&metadata, "mpris:artUrl").map(str::to_owned))
        };
        let art = match (url, &player.art) {
            (None, _) => None,
            (Some(url), Some((seen, art))) if *seen == url => art.clone(),
            (Some(url), _) => {
                let art = resolve_art(&url).await;
                player.art = Some((url, art.clone()));
                art
            }
        };
        let snapshot = player.mirror.borrow();
        let state = props::lookup(&snapshot.objects, MPRIS_PATH, MPRIS_PLAYER);
        let root = props::lookup(&snapshot.objects, MPRIS_PATH, MPRIS_ROOT);
        players.push((
            read_player(
                name,
                state.unwrap_or(&Props::new()),
                root.unwrap_or(&Props::new()),
                art,
            ),
            state.is_some(),
        ));
    }
    if chosen
        .as_ref()
        .is_some_and(|name| !players.iter().any(|(player, _)| &player.bus_name == name))
    {
        *chosen = None;
    }
    let current = chosen
        .as_ref()
        .and_then(|name| {
            players
                .iter()
                .position(|(player, _)| &player.bus_name == name)
        })
        .or_else(|| (!players.is_empty()).then_some(0));
    settled &= current.is_none_or(|current| players[current].1);
    MediaState {
        players: players.into_iter().map(|(player, _)| player).collect(),
        current,
        refused,
        settled,
    }
}

/// A command being carried out.
type Running = Pin<Box<dyn Future<Output = zbus::Result<()>> + Send>>;

/// Every call allows interactive authorisation, as the bar's calls to a player always did,
/// and is bounded by `call_timeout`.
async fn execute(
    connection: Connection,
    player: String,
    command: MediaCommand,
    call_timeout: Duration,
) -> zbus::Result<()> {
    let call = async {
        let proxy = Builder::<'_, zbus::Proxy<'_>>::new(&connection)
            .destination(player)?
            .path(MPRIS_PATH)?
            .interface(MPRIS_PLAYER)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let interactive = MethodFlags::AllowInteractiveAuth.into();
        match command {
            MediaCommand::PlayPause => {
                proxy
                    .call_with_flags::<_, _, ()>("PlayPause", interactive, &())
                    .await
            }
            MediaCommand::Next => {
                proxy
                    .call_with_flags::<_, _, ()>("Next", interactive, &())
                    .await
            }
            MediaCommand::Previous => {
                proxy
                    .call_with_flags::<_, _, ()>("Previous", interactive, &())
                    .await
            }
            MediaCommand::Seek(offset) => {
                proxy
                    .call_with_flags::<_, _, ()>("Seek", interactive, &(offset,))
                    .await
            }
            MediaCommand::Choose(_) => Ok(None),
        }
    };
    timeout(call_timeout, call)
        .await
        .map_err(|_| zbus::Error::Failure("no answer in time".into()))?
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::runtime::Handle;
    use tokio::sync::{mpsc, watch};
    use zbus::message::Header;
    use zbus::zvariant::{OwnedValue, Value};
    use zbus::Connection;

    use super::*;
    use crate::testbus::{self, TestBus};

    fn owned(value: Value<'_>) -> OwnedValue {
        value.try_to_owned().expect("owned value")
    }

    fn dict(entries: Vec<(&str, Value<'_>)>) -> OwnedValue {
        owned(Value::new(entries.into_iter().collect::<HashMap<_, _>>()))
    }

    fn props(entries: Vec<(&str, OwnedValue)>) -> Props {
        entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    }

    #[test]
    fn the_track_is_sanitised_and_needs_a_title() {
        let player = |metadata: OwnedValue| {
            props(vec![
                ("Metadata", metadata),
                ("PlaybackStatus", owned(Value::from("Playing"))),
            ])
        };
        let full = player(dict(vec![
            ("xesam:title", Value::from("Night\u{202e} Drive")),
            ("xesam:artist", Value::new(vec!["Calmo", "Duo"])),
        ]));
        assert_eq!(
            track(&full),
            Some(Track {
                title: "Night Drive".into(),
                artist: Some("Calmo, Duo".into())
            })
        );
        assert!(playing(&full));
        let no_title = player(dict(vec![("xesam:artist", Value::new(vec!["Calmo"]))]));
        assert_eq!(track(&no_title), None);
        assert_eq!(
            track(&player(dict(vec![("xesam:title", Value::from(42u32))]))),
            None
        );
        assert!(is_player("org.mpris.MediaPlayer2.athanor"));
        assert!(!is_player("org.mpris.MediaPlayer2."));
        assert!(!is_player("org.example.Player"));
    }

    #[test]
    fn a_property_of_the_wrong_type_reads_as_absent_instead_of_panicking() {
        let wrong = props(vec![
            ("Metadata", owned(Value::from(42u32))),
            ("PlaybackStatus", owned(Value::from(7u32))),
            ("CanSeek", owned(Value::from("yes"))),
            ("Position", owned(Value::from("0"))),
        ]);
        assert_eq!(track(&wrong), None);
        assert!(!playing(&wrong));
        let player = read_player("org.mpris.MediaPlayer2.x", &wrong, &Props::new(), None);
        assert!(!player.can_seek && !player.can_next && !player.can_previous);
        assert_eq!((player.position_us, player.length_us), (None, None));
        assert_eq!(
            player.identity, "x",
            "the name stands in for a missing Identity"
        );
    }

    #[test]
    fn the_length_is_read_from_the_metadata_in_either_integer_type() {
        let length = |value: Value<'_>| {
            let player = props(vec![("Metadata", dict(vec![("mpris:length", value)]))]);
            read_player("org.mpris.MediaPlayer2.x", &player, &Props::new(), None).length_us
        };
        assert_eq!(length(Value::from(5_000_000i64)), Some(5_000_000));
        assert_eq!(length(Value::from(5_000_000u64)), Some(5_000_000));
        assert_eq!(length(Value::from(u64::MAX)), None);
        assert_eq!(length(Value::from("5")), None);
    }

    #[test]
    fn only_a_file_uri_names_a_path_and_it_is_decoded() {
        let path = |uri: &str| file_path(uri);
        assert_eq!(
            path("file:///music/a%20b/c%C3%A9.png"),
            Some(PathBuf::from("/music/a b/cé.png"))
        );
        assert_eq!(
            path("file://localhost/x.png"),
            Some(PathBuf::from("/x.png"))
        );
        for bad in [
            "file://example.org/x.png",
            "file:/x.png",
            "file://x.png",
            "file:///a%00b",
            "file:///a%zz",
            "file:///a%2",
            "file:///a?x",
            "file:///a#x",
            "https://example.org/x.png",
            "x.png",
            "",
        ] {
            assert_eq!(path(bad), None, "{bad}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artwork_is_a_regular_file_or_a_small_data_uri_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("athanor-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cover.png");
        std::fs::write(&file, b"png").unwrap();
        let fifo = dir.join("fifo.png");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.is_ok_and(|status| status.success()), "mkfifo");
        let uri = |path: &std::path::Path| format!("file://{}", path.display());
        assert_eq!(
            resolve_art(&uri(&file)).await,
            Some(Art::Data(b"png".to_vec().into()))
        );
        let at_cap = dir.join("cap.png");
        std::fs::write(&at_cap, vec![7u8; MAX_ART_BYTES]).unwrap();
        let over = dir.join("over.png");
        std::fs::write(&over, vec![7u8; MAX_ART_BYTES + 1]).unwrap();
        assert!(matches!(
            resolve_art(&uri(&at_cap)).await,
            Some(Art::Data(bytes)) if bytes.len() == MAX_ART_BYTES
        ));
        assert_eq!(resolve_art(&uri(&over)).await, None, "over the size limit");
        // The reader itself, as the model calls it: a FIFO is refused without blocking.
        assert_eq!(read_art_file(&fifo), None);
        assert_eq!(
            resolve_art(&uri(&fifo)).await,
            None,
            "a FIFO would hang an open"
        );
        assert_eq!(resolve_art(&uri(&dir)).await, None, "a directory");
        assert_eq!(resolve_art(&uri(&dir.join("gone.png"))).await, None);
        assert_eq!(
            resolve_art(&uri(std::path::Path::new("/dev/zero"))).await,
            None
        );
        assert_eq!(resolve_art("https://example.org/cover.png").await, None);
        assert_eq!(resolve_art("http://example.org/cover.png").await, None);
        assert_eq!(
            resolve_art("data:image/png;base64,cG5n").await,
            Some(Art::Data(b"png".to_vec().into()))
        );
        assert_eq!(
            resolve_art("data:image/png;base64,cA==").await,
            Some(Art::Data(b"p".to_vec().into()))
        );
        assert_eq!(resolve_art("data:image/png;base64,c*Bn").await, None);
        assert_eq!(resolve_art("data:image/png,png").await, None, "not base64");
        assert_eq!(
            resolve_art("data:text/html;base64,cG5n").await,
            None,
            "not an image"
        );
        let big = format!(
            "data:image/png;base64,{}",
            "AAAA".repeat(MAX_ART_BYTES / 3 + 4)
        );
        assert_eq!(resolve_art(&big).await, None, "over the size limit");
        let just_over = format!(
            "data:image/png;base64,{}",
            "AAAA".repeat(MAX_ART_BYTES / 3 + 1)
        );
        assert_eq!(
            resolve_art(&just_over).await,
            None,
            "decoded size over the limit"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        interactive: Vec<String>,
        plain: Vec<String>,
        refuse: Vec<&'static str>,
        hold: Vec<&'static str>,
    }

    type Shared = Arc<Mutex<Log>>;

    struct Root {
        identity: String,
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        #[zbus(property)]
        fn identity(&self) -> String {
            self.identity.clone()
        }
    }

    struct FakePlayer {
        tag: String,
        status: OwnedValue,
        metadata: OwnedValue,
        can_next: OwnedValue,
        can_previous: OwnedValue,
        can_seek: OwnedValue,
        position: OwnedValue,
        log: Shared,
    }

    impl FakePlayer {
        async fn call(&self, what: String, header: &Header<'_>) -> zbus::fdo::Result<()> {
            let interactive = header
                .primary()
                .flags()
                .contains(zbus::message::Flags::AllowInteractiveAuth);
            let (hold, refuse) = {
                let mut log = self.log.lock().unwrap();
                let line = format!("{} {what}", self.tag);
                log.calls.push(line.clone());
                if interactive {
                    log.interactive.push(line);
                } else {
                    log.plain.push(line);
                }
                let method = what.split(' ').next().unwrap_or_default();
                (log.hold.contains(&method), log.refuse.contains(&method))
            };
            if hold {
                std::future::pending::<()>().await;
            }
            if refuse {
                return Err(zbus::fdo::Error::Failed("refused".into()));
            }
            Ok(())
        }
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl FakePlayer {
        #[zbus(property)]
        fn playback_status(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.status.try_clone().map_err(zbus::Error::from)?)
        }
        #[zbus(property)]
        fn metadata(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.metadata.try_clone().map_err(zbus::Error::from)?)
        }
        #[zbus(property)]
        fn can_go_next(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.can_next.try_clone().map_err(zbus::Error::from)?)
        }
        #[zbus(property)]
        fn can_go_previous(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.can_previous.try_clone().map_err(zbus::Error::from)?)
        }
        #[zbus(property)]
        fn can_seek(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.can_seek.try_clone().map_err(zbus::Error::from)?)
        }
        #[zbus(property)]
        fn position(&self) -> zbus::fdo::Result<OwnedValue> {
            Ok(self.position.try_clone().map_err(zbus::Error::from)?)
        }
        async fn play_pause(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
            self.call("PlayPause".into(), &header).await
        }
        async fn next(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
            self.call("Next".into(), &header).await
        }
        async fn previous(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
            self.call("Previous".into(), &header).await
        }
        async fn seek(
            &self,
            offset: i64,
            #[zbus(header)] header: Header<'_>,
        ) -> zbus::fdo::Result<()> {
            self.call(format!("Seek {offset}"), &header).await
        }
    }

    fn fake(tag: &str, log: &Shared) -> FakePlayer {
        FakePlayer {
            tag: tag.to_owned(),
            status: owned(Value::from("Paused")),
            metadata: dict(vec![("xesam:title", Value::from(format!("Song of {tag}")))]),
            can_next: owned(Value::from(true)),
            can_previous: owned(Value::from(false)),
            can_seek: owned(Value::from(true)),
            position: owned(Value::from(1_500_000i64)),
            log: log.clone(),
        }
    }

    const PREFIX: &str = "org.mpris.MediaPlayer2.";

    async fn serve_player(bus: &TestBus, suffix: &str, player: FakePlayer) -> Connection {
        let build = bus
            .builder()
            .serve_at(
                MPRIS_PATH,
                Root {
                    identity: format!("Identity {suffix}"),
                },
            )
            .expect("root")
            .serve_at(MPRIS_PATH, player)
            .expect("player")
            .name(format!("{PREFIX}{suffix}"))
            .expect("name")
            .build();
        tokio::time::timeout(testbus::WAIT, build)
            .await
            .expect("the player connected in time")
            .expect("player")
    }

    async fn set_title(server: &Connection, title: &str) {
        let object = server
            .object_server()
            .interface::<_, FakePlayer>(MPRIS_PATH)
            .await
            .expect("player object");
        object.get_mut().await.metadata = dict(vec![("xesam:title", Value::from(title))]);
        object
            .get()
            .await
            .metadata_changed(object.signal_emitter())
            .await
            .expect("PropertiesChanged");
    }

    struct Rig {
        bus: TestBus,
        _client: Connection,
        states: watch::Receiver<MediaState>,
        commands: mpsc::UnboundedSender<MediaCommand>,
        log: Shared,
    }

    async fn rig_with(call_timeout: Duration, ready: impl FnOnce(&Shared)) -> Rig {
        let log = Shared::default();
        ready(&log);
        let bus = TestBus::start();
        let client = bus.client().await;
        let (states, commands) = spawn_with(
            &Handle::current(),
            Buses::with(client.clone()),
            call_timeout,
        );
        Rig {
            bus,
            _client: client,
            states,
            commands,
            log,
        }
    }

    async fn rig() -> Rig {
        rig_with(TIMEOUT, |_| ()).await
    }

    async fn wait_for(
        rx: &mut watch::Receiver<MediaState>,
        predicate: impl Fn(&MediaState) -> bool,
    ) -> MediaState {
        tokio::time::timeout(testbus::WAIT, rx.wait_for(|state| predicate(state)))
            .await
            .expect("the model reached the expected state in time")
            .expect("the model is running")
            .clone()
    }

    async fn until(log: &Shared, predicate: impl Fn(&Log) -> bool) {
        tokio::time::timeout(testbus::WAIT, async {
            while !predicate(&log.lock().unwrap()) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the player saw what the test waits for");
    }

    fn names(state: &MediaState) -> Vec<&str> {
        state.players.iter().map(|p| p.bus_name.as_str()).collect()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_no_player_the_model_is_settled_with_an_empty_list() {
        let mut rig = rig().await;
        let state = wait_for(&mut rig.states, |state| state.settled).await;
        assert!(state.players.is_empty() && state.current.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_player_is_listed_with_what_the_controls_show() {
        let mut rig = rig().await;
        let mut player = fake("a", &rig.log);
        player.status = owned(Value::from("Playing"));
        player.metadata = dict(vec![
            ("xesam:title", Value::from("Night Drive")),
            ("xesam:artist", Value::new(vec!["Calmo"])),
            ("mpris:length", Value::from(200_000_000i64)),
        ]);
        let _server = serve_player(&rig.bus, "a", player).await;
        let state = wait_for(&mut rig.states, |state| {
            state.settled && !state.players.is_empty()
        })
        .await;
        assert_eq!(state.current, Some(0));
        let player = &state.players[0];
        assert_eq!(player.bus_name, "org.mpris.MediaPlayer2.a");
        assert_eq!(player.identity, "Identity a");
        assert_eq!(
            player.track,
            Some(Track {
                title: "Night Drive".into(),
                artist: Some("Calmo".into())
            })
        );
        assert!(player.playing && player.can_next && player.can_seek && !player.can_previous);
        assert_eq!(player.position_us, Some(1_500_000));
        assert_eq!(player.length_us, Some(200_000_000));
        assert_eq!(player.art, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artwork_of_a_file_uri_is_a_path_and_an_https_one_is_absent() {
        let mut rig = rig().await;
        let dir = std::env::temp_dir().join(format!("athanor-media-bus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cover = dir.join("cover art.png");
        std::fs::write(&cover, b"png").unwrap();
        let mut local = fake("a", &rig.log);
        local.metadata = dict(vec![
            ("xesam:title", Value::from("Local")),
            (
                "mpris:artUrl",
                Value::from(format!(
                    "file://{}",
                    cover.display().to_string().replace(' ', "%20")
                )),
            ),
        ]);
        let mut remote = fake("b", &rig.log);
        remote.metadata = dict(vec![
            ("xesam:title", Value::from("Remote")),
            ("mpris:artUrl", Value::from("https://example.org/cover.png")),
        ]);
        let _a = serve_player(&rig.bus, "a", local).await;
        let _b = serve_player(&rig.bus, "b", remote).await;
        let state = wait_for(&mut rig.states, |state| {
            state.players.len() == 2 && state.players.iter().all(|p| p.track.is_some())
        })
        .await;
        assert_eq!(
            state.players[0].art,
            Some(Art::Data(b"png".to_vec().into()))
        );
        assert_eq!(state.players[1].art, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn players_come_and_go_and_the_first_by_name_is_current() {
        let mut rig = rig().await;
        let _b = serve_player(&rig.bus, "b", fake("b", &rig.log)).await;
        let state = wait_for(&mut rig.states, |state| {
            state.settled && state.players.len() == 1
        })
        .await;
        assert_eq!(names(&state), [format!("{PREFIX}b")]);
        let a = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        let state = wait_for(&mut rig.states, |state| state.players.len() == 2).await;
        assert_eq!(names(&state), [format!("{PREFIX}a"), format!("{PREFIX}b")]);
        assert_eq!(state.current, Some(0));
        a.release_name(format!("{PREFIX}a")).await.unwrap();
        let state = wait_for(&mut rig.states, |state| state.players.len() == 1).await;
        assert_eq!(names(&state), [format!("{PREFIX}b")]);
        assert_eq!(state.current, Some(0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_chosen_player_is_current_until_it_leaves() {
        let mut rig = rig().await;
        let _a = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        let b = serve_player(&rig.bus, "b", fake("b", &rig.log)).await;
        wait_for(&mut rig.states, |state| {
            state.settled && state.players.len() == 2
        })
        .await;
        rig.commands
            .send(MediaCommand::Choose(format!("{PREFIX}missing")))
            .unwrap();
        rig.commands
            .send(MediaCommand::Choose(format!("{PREFIX}b")))
            .unwrap();
        wait_for(&mut rig.states, |state| state.current == Some(1)).await;
        b.release_name(format!("{PREFIX}b")).await.unwrap();
        let state = wait_for(&mut rig.states, |state| state.players.len() == 1).await;
        assert_eq!(state.current, Some(0));
        let _b = serve_player(&rig.bus, "b", fake("b", &rig.log)).await;
        let state = wait_for(&mut rig.states, |state| state.players.len() == 2).await;
        assert_eq!(state.current, Some(0), "the choice ended with the player");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_players_own_changes_are_followed_and_a_strangers_are_not() {
        let mut rig = rig().await;
        let server = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        wait_for(&mut rig.states, |state| {
            state.settled && !state.players.is_empty()
        })
        .await;
        // Another peer signals the player's path: the model follows the owner's signals only.
        let stranger = rig.bus.client().await;
        let emitter = zbus::object_server::SignalEmitter::new(&stranger, MPRIS_PATH).unwrap();
        let forged = HashMap::from([("PlaybackStatus", Value::from("Playing"))]);
        emitter
            .emit(
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(MPRIS_PLAYER, forged, Vec::<&str>::new()),
            )
            .await
            .unwrap();
        set_title(&server, "Second").await;
        let state = wait_for(&mut rig.states, |state| {
            state.players[0]
                .track
                .as_ref()
                .is_some_and(|t| t.title == "Second")
        })
        .await;
        assert!(!state.players[0].playing, "the forged signal was ignored");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_player_whose_properties_have_the_wrong_type_is_listed_without_a_panic() {
        let mut rig = rig().await;
        let mut player = fake("a", &rig.log);
        player.status = owned(Value::from(3u32));
        player.metadata = owned(Value::from("not a dict"));
        player.can_next = owned(Value::from("yes"));
        player.position = owned(Value::from(true));
        let _a = serve_player(&rig.bus, "a", player).await;
        let state = wait_for(&mut rig.states, |state| {
            state.settled && !state.players.is_empty()
        })
        .await;
        let player = &state.players[0];
        assert_eq!(
            (player.track.clone(), player.playing, player.can_next),
            (None, false, false)
        );
        assert_eq!(player.position_us, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn at_most_the_cap_of_players_are_followed() {
        let mut rig = rig().await;
        let mut servers = Vec::new();
        for index in 0..MAX_PLAYERS + 2 {
            let suffix = format!("p{index:02}");
            servers.push(serve_player(&rig.bus, &suffix, fake(&suffix, &rig.log)).await);
        }
        wait_for(&mut rig.states, |state| state.players.len() == MAX_PLAYERS).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(rig.states.borrow().players.len(), MAX_PLAYERS);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_player_turned_away_at_the_cap_is_followed_when_one_leaves() {
        let mut rig = rig().await;
        let mut servers = Vec::new();
        for index in 0..MAX_PLAYERS {
            let suffix = format!("p{index:02}");
            servers.push(serve_player(&rig.bus, &suffix, fake(&suffix, &rig.log)).await);
        }
        wait_for(&mut rig.states, |state| state.players.len() == MAX_PLAYERS).await;
        let late = serve_player(&rig.bus, "z", fake("z", &rig.log)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!names(&rig.states.borrow()).contains(&"org.mpris.MediaPlayer2.z"));
        let gone = servers.remove(0);
        gone.release_name(format!("{PREFIX}p00")).await.unwrap();
        wait_for(&mut rig.states, |state| {
            state
                .players
                .iter()
                .any(|p| p.bus_name == format!("{PREFIX}z"))
        })
        .await;
        drop(late);
    }

    #[test]
    fn a_cloned_state_shares_the_artwork_bytes() {
        let state = MediaState {
            players: vec![Player {
                bus_name: "org.mpris.MediaPlayer2.a".into(),
                identity: "a".into(),
                track: None,
                playing: false,
                can_next: false,
                can_previous: false,
                can_seek: false,
                position_us: None,
                length_us: None,
                art: Some(Art::Data(vec![1, 2, 3].into())),
            }],
            ..MediaState::default()
        };
        let copy = state.clone();
        let (Some(Art::Data(a)), Some(Art::Data(b))) =
            (&state.players[0].art, &copy.players[0].art)
        else {
            panic!("artwork");
        };
        assert!(std::sync::Arc::ptr_eq(a, b));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn commands_reach_the_current_player_with_interactive_authorization() {
        let mut rig = rig().await;
        let _a = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        let _b = serve_player(&rig.bus, "b", fake("b", &rig.log)).await;
        wait_for(&mut rig.states, |state| {
            state.settled && state.players.len() == 2
        })
        .await;
        for command in [
            MediaCommand::PlayPause,
            MediaCommand::Next,
            MediaCommand::Previous,
            MediaCommand::Seek(-5_000_000),
        ] {
            rig.commands.send(command).unwrap();
        }
        until(&rig.log, |log| log.calls.len() >= 4).await;
        let log = rig.log.lock().unwrap();
        assert!(log.plain.is_empty(), "without the flag: {:?}", log.plain);
        assert_eq!(
            log.interactive,
            ["a PlayPause", "a Next", "a Previous", "a Seek -5000000"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_command_is_counted_once_and_not_tried_again() {
        let mut rig = rig_with(TIMEOUT, |log| {
            log.lock().unwrap().refuse.push("Next");
        })
        .await;
        let _a = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        wait_for(&mut rig.states, |state| {
            state.settled && !state.players.is_empty()
        })
        .await;
        rig.commands.send(MediaCommand::Next).unwrap();
        wait_for(&mut rig.states, |state| state.refused == 1).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(rig.log.lock().unwrap().calls, ["a Next"]);
        assert_eq!(rig.states.borrow().refused, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_player_that_does_not_answer_holds_the_queue_for_the_timeout_only() {
        let mut rig = rig_with(Duration::from_millis(300), |log| {
            log.lock().unwrap().hold.push("PlayPause");
        })
        .await;
        let _a = serve_player(&rig.bus, "a", fake("a", &rig.log)).await;
        wait_for(&mut rig.states, |state| {
            state.settled && !state.players.is_empty()
        })
        .await;
        rig.commands.send(MediaCommand::PlayPause).unwrap();
        // Waiting while the first call is in flight: the newest seek replaces the older ones.
        rig.commands.send(MediaCommand::Seek(1)).unwrap();
        rig.commands.send(MediaCommand::Seek(2)).unwrap();
        rig.commands.send(MediaCommand::Seek(3)).unwrap();
        rig.commands.send(MediaCommand::Next).unwrap();
        wait_for(&mut rig.states, |state| state.refused == 1).await;
        until(&rig.log, |log| log.calls.len() >= 3).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            rig.log.lock().unwrap().calls,
            ["a PlayPause", "a Seek 3", "a Next"]
        );
    }
}
