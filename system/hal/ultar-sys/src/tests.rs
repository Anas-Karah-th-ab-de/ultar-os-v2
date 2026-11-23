//! Tests of the wrappers (safe code only; the `unsafe` blocks all live in
//! `raw.rs`).

use super::*;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

#[test]
fn peer_credentials_of_a_socket_pair_are_our_own() {
    let (left, _right) = UnixStream::pair().unwrap();
    let cred = peer_credentials(left.as_fd()).unwrap();
    assert_eq!(cred.uid, euid());
    assert_eq!(cred.gid, gid());
    assert_eq!(cred.pid, std::process::id());
}

#[test]
fn waiting_reports_which_descriptor_is_readable() {
    use std::io::Write;
    let (quiet_reader, _quiet_writer) = std::io::pipe().unwrap();
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer.write_all(b"x").unwrap();
    assert_eq!(
        wait_readable(quiet_reader.as_fd(), reader.as_fd()),
        Ok((false, true))
    );
    assert_eq!(
        wait_readable(reader.as_fd(), quiet_reader.as_fd()),
        Ok((true, false))
    );
    // A closed writer makes its reader "ready" as well (end of file).
    let (closed_reader, closed_writer) = std::io::pipe().unwrap();
    drop(closed_writer);
    assert_eq!(
        wait_readable(closed_reader.as_fd(), reader.as_fd()),
        Ok((true, true))
    );
}

#[test]
fn peer_credentials_of_a_non_socket_fail() {
    let file = std::fs::File::open("/").unwrap();
    assert!(peer_credentials(file.as_fd()).is_err());
}

#[test]
fn limits_can_be_read() {
    let (soft, hard) = limit(Resource::OpenFiles).unwrap();
    assert!(soft > 0 && soft <= hard);
    for resource in [Resource::Core, Resource::Processes, Resource::AddressSpace] {
        let (soft, hard) = limit(resource).unwrap();
        assert!(soft <= hard);
    }
}

#[test]
fn ids_match_the_metadata_of_an_own_file() {
    use std::os::unix::fs::MetadataExt;
    let path = std::env::temp_dir().join(format!("ultar-sys-{}", std::process::id()));
    std::fs::write(&path, b"x").unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    assert_eq!(meta.uid(), euid());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(uid(), euid());
}

#[test]
fn the_ultar_right_options_are_unknown_to_linux() {
    // The runtime uses exactly this to tell the two kernels apart.
    assert_eq!(ultar_rights(), Err(Errno::EINVAL));
    assert_eq!(ultar_drop_rights(0), Err(Errno::EINVAL));
}

#[test]
fn limits_round_trip_and_the_mask_is_returned() {
    // Setting a limit to its current value changes nothing.
    let (soft, hard) = limit(Resource::Core).unwrap();
    set_limit(Resource::Core, soft, hard).unwrap();
    assert_eq!(limit(Resource::Core).unwrap(), (soft, hard));
    // Raising a hard limit is refused for an unprivileged process only;
    // lowering the soft limit below the hard one always works.
    let (soft, hard) = limit(Resource::OpenFiles).unwrap();
    set_limit(Resource::OpenFiles, soft.min(hard), hard).unwrap();
    let old = set_umask(0o027);
    assert_eq!(set_umask(old), 0o027);
    assert_eq!(set_umask(old), old);
}

#[test]
fn adopt_refuses_standard_streams_closed_numbers_and_second_owners() {
    assert_eq!(adopt_fd(1).err(), Some(Errno::EINVAL));
    assert_eq!(adopt_fd(-4).err(), Some(Errno::EINVAL));
    assert_eq!(adopt_fd(999_983).err(), Some(Errno::EBADF));
    assert!(!is_open(999_983));
    assert!(!is_open(-1));
}

#[test]
fn error_numbers_are_decoded() {
    assert_eq!(Errno::from(rustix::io::Errno::INVAL), Errno::EINVAL);
    assert_eq!(Errno::from(rustix::io::Errno::PERM), Errno::EPERM);
    assert!(!Errno::EPERM.to_string().is_empty());
}
