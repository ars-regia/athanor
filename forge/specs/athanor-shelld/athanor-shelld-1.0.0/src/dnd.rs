//! The do-not-disturb switch, the only thing the daemon keeps across sessions (BR4): the file
//! `do-not-disturb` in the daemon's state directory, present when the switch is on. It never
//! holds a notification.

use std::fs;
use std::io;
use std::path::Path;

const FILE: &str = "do-not-disturb";

pub fn load(dir: &Path) -> io::Result<bool> {
    dir.join(FILE).try_exists()
}

pub fn save(dir: &Path, on: bool) -> io::Result<()> {
    let path = dir.join(FILE);
    if on {
        return fs::write(path, b"on\n");
    }
    match fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_round_trips_and_off_twice_is_fine() {
        let dir = std::env::temp_dir().join(format!("athanor-shelld-dnd-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        assert!(!load(&dir).expect("load"));
        save(&dir, true).expect("on");
        assert!(load(&dir).expect("load"));
        save(&dir, false).expect("off");
        save(&dir, false).expect("off again");
        assert!(!load(&dir).expect("load"));
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
