//! Cells the launcher no longer has a stage 1 for (review L-3): a stop
//! whose cell did not end, and cells left from a previous run of the
//! launcher (it was ended or crashed). They are found by their **memory
//! group** `apps/<app>` (one group per role below it), never by a
//! remembered process id.

use crate::appid::AppId;
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_init::spec::Slice;
use ultar_sys::cell::{send_signal, StopSignal};

/// How long the members of a group get to disappear.
const EMPTY_WAIT: Duration = Duration::from_secs(5);
/// Pause between two looks at a group's members.
const LOOK: Duration = Duration::from_millis(20);

/// The members of the group at `dir` and of the groups right below it (the
/// roles of an app).
fn members_below(dir: &Path) -> Vec<u32> {
    let mut all = ultar_init::groups::members(dir);
    for sub in subgroups(dir) {
        all.extend(ultar_init::groups::members(&sub));
    }
    all
}

fn subgroups(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Removes the group at `dir` with the role groups below it (empty ones;
/// a group with processes stays).
pub fn remove_group(dir: &Path) {
    for sub in subgroups(dir) {
        ultar_init::groups::remove(&sub);
    }
    ultar_init::groups::remove(dir);
}

/// Ends every process of the group at `dir` and of the groups below it:
/// `cgroup.kill` (Linux and the Ultar kernel end the whole subtree at once,
/// with no process number in between), else `SIGKILL` to each member as the
/// groups list them at that moment. Returns whether they are empty
/// afterwards (within [`EMPTY_WAIT`]).
pub fn end_group(dir: &Path) -> bool {
    let deadline = Instant::now() + EMPTY_WAIT;
    // only the kernel's file: never create one
    let killed = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("cgroup.kill"))
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"1"))
        .is_ok();
    loop {
        let members = members_below(dir);
        if members.is_empty() {
            return true;
        }
        if !killed {
            for pid in &members {
                let _ = send_signal(*pid, StopSignal::Kill);
            }
        }
        if Instant::now() > deadline {
            return false;
        }
        // waiting for the kernel to end them: nothing to be woken by
        std::thread::sleep(LOOK);
    }
}

/// Ends the cells of `apps` left in the group tree at `root` (a previous
/// run of the launcher). Returns the apps whose group could not be emptied.
pub fn end_leftovers<'a>(root: &Path, apps: impl Iterator<Item = &'a AppId>) -> Vec<AppId> {
    apps.filter(|id| {
        let dir = root.join(Slice::Apps.dir()).join(id.as_str());
        dir.is_dir() && !members_below(&dir).is_empty() && !end_group(&dir)
    })
    .cloned()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_of_plain_files_is_emptied_by_its_members() {
        let dir = std::env::temp_dir().join(format!("ulaunch-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let group = dir.join("apps/x");
        std::fs::create_dir_all(&group).unwrap();
        // an empty group is empty
        std::fs::write(group.join("cgroup.procs"), "").unwrap();
        assert!(end_group(&group));
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        std::fs::write(group.join("cgroup.procs"), format!("{}\n", child.id())).unwrap();
        // a fake tree does not forget the member: not emptied, but it was killed
        let id = AppId::new("x").unwrap();
        let left = end_leftovers(&dir, [&id].into_iter());
        assert_eq!(left, vec![id]);
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
