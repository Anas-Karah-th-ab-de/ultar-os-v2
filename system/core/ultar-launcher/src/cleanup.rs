//! Cells the launcher no longer has a stage 1 for (review L-3): a stop
//! whose cell did not end, and cells left from a previous run of the
//! launcher (it was ended or crashed). They are found by their **memory
//! group** `apps/<app>`, never by a remembered process id.

use crate::appid::AppId;
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_init::spec::Slice;
use ultar_sys::cell::{send_signal, StopSignal};

/// How long the members of a group get to disappear.
const EMPTY_WAIT: Duration = Duration::from_secs(5);
/// Pause between two looks at a group's members.
const LOOK: Duration = Duration::from_millis(20);

/// Ends every process of the group at `dir`: `cgroup.kill` where the
/// kernel has it (Linux), else `SIGKILL` to each member as the group lists
/// it at that moment (the Ultar kernel: the list is read right before, its
/// numbers belong to members of the group). Returns whether the group is
/// empty afterwards (within [`EMPTY_WAIT`]).
pub fn end_group(dir: &Path) -> bool {
    let deadline = Instant::now() + EMPTY_WAIT;
    // only the kernel's file: never create one
    let killed = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("cgroup.kill"))
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"1"))
        .is_ok();
    loop {
        let members = ultar_init::groups::members(dir);
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
        dir.is_dir() && !ultar_init::groups::members(&dir).is_empty() && !end_group(&dir)
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
