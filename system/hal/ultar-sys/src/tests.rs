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
fn inherited_descriptors_are_checked_and_taken_only_once() {
    // The only test of this binary that calls `take_inherited`: it works
    // once per process.
    use std::os::fd::IntoRawFd;
    let (reader, writer) = std::io::pipe().unwrap();
    let pipe_fd = reader.into_raw_fd();
    let socket = UnixStream::pair().unwrap().0.into_raw_fd();
    // A socket is not an acceptable inherited descriptor.
    let refused = take_inherited(&[pipe_fd, socket], false, 0);
    assert_eq!(refused.err(), Some(InheritError::WrongType(socket)));
    // The first call is spent – also when it failed.
    let again = take_inherited(&[pipe_fd], false, 0);
    assert_eq!(again.err(), Some(InheritError::AlreadyTaken));
    drop(writer);
    for error in [
        InheritError::AlreadyTaken,
        InheritError::Reserved(1),
        InheritError::NotOpen(9),
        InheritError::WrongType(5),
        InheritError::Kernel(4, Errno::EBADF),
    ] {
        assert!(!error.to_string().is_empty());
        assert_eq!(error.fd().is_none(), error == InheritError::AlreadyTaken);
    }
    assert!(!is_open(999_983));
    assert!(!is_open(-1));
}

#[test]
fn polling_reports_readable_descriptors_and_honours_the_timeout() {
    use std::io::Write;
    let (quiet, _keep) = std::io::pipe().unwrap();
    let (ready, mut writer) = std::io::pipe().unwrap();
    let idle = poll_readable(&[quiet.as_fd()], Some(std::time::Duration::from_millis(10)));
    assert_eq!(idle, Ok(vec![false]));
    writer.write_all(b"x").unwrap();
    let both = poll_readable(&[quiet.as_fd(), ready.as_fd()], None).unwrap();
    assert_eq!(both, vec![false, true]);
}

#[test]
fn error_numbers_are_decoded() {
    assert_eq!(Errno::from(rustix::io::Errno::INVAL), Errno::EINVAL);
    assert_eq!(Errno::from(rustix::io::Errno::PERM), Errno::EPERM);
    assert!(!Errno::EPERM.to_string().is_empty());
}

#[test]
fn descriptors_can_be_marked_as_inherited_and_back() {
    let (reader, _writer) = std::io::pipe().unwrap();
    assert_eq!(is_inherited(&reader), Ok(false));
    set_inherited(&reader, true).unwrap();
    assert_eq!(is_inherited(&reader), Ok(true));
    set_inherited(&reader, false).unwrap();
    assert_eq!(is_inherited(&reader), Ok(false));
}

#[test]
fn ids_are_switched_and_read_back_or_refused() {
    // The calls change the calling thread only: a thread of its own keeps
    // the test process as it was.
    let outcome = std::thread::spawn(|| {
        let nobody = Identity {
            uid: 65534,
            gid: 65534,
            groups: vec![65534],
        };
        let first = switch_identity(&nobody);
        // Back to root is refused once the ids are dropped.
        let back = switch_identity(&Identity::default());
        (first, back, uid())
    })
    .join()
    .unwrap();
    if euid() == 0 {
        assert_eq!(outcome.0, Ok(()));
        assert_eq!(outcome.1, Err(IdentityError::Kernel(Errno::EPERM)));
    } else {
        assert_eq!(outcome.0, Err(IdentityError::Kernel(Errno::EPERM)));
    }
    let many = Identity {
        groups: (0..=MAX_GROUPS as u32).collect(),
        ..Identity::default()
    };
    assert_eq!(switch_identity(&many), Err(IdentityError::TooManyGroups));
    assert!(!IdentityError::NotApplied.to_string().is_empty());
}

#[test]
fn the_signal_mask_is_replaced_and_restored() {
    std::thread::spawn(|| {
        let before = set_signal_mask(1 << 9).unwrap();
        assert_eq!(set_signal_mask(before).unwrap(), 1 << 9);
    })
    .join()
    .unwrap();
}

#[test]
fn uevent_messages_are_parsed_and_foreign_ones_skipped() {
    use crate::uevent::Uevent;
    let msg = b"bind@/bus/pci/devices/0000:00:04.0\0ACTION=bind\0DEVPATH=/bus/pci/devices/0000:00:04.0\0SUBSYSTEM=pci\0SEQNUM=17\0DRIVER=gen-intel-eth\0";
    let event = Uevent::parse(msg).unwrap();
    assert_eq!(event.action, "bind");
    assert_eq!(event.devpath, "/bus/pci/devices/0000:00:04.0");
    assert_eq!(event.subsystem, "pci");
    assert_eq!(event.seqnum, Some(17));
    assert_eq!(event.var("DRIVER"), Some("gen-intel-eth"));
    assert_eq!(event.var("MISSING"), None);
    // the Linux device manager's re-broadcast and garbage are no kernel events
    assert_eq!(Uevent::parse(b"libudev\0\xfe\xed\xca\xfe"), None);
    assert_eq!(Uevent::parse(b""), None);
    assert_eq!(Uevent::parse(b"\xff\xfe@x\0"), None);
    let no_seq = Uevent::parse(b"add@/x\0SEQNUM=nope\0broken\0").unwrap();
    assert_eq!((no_seq.seqnum, no_seq.vars.len()), (None, 1));
}

#[test]
fn uevent_socket_opens_or_says_why() {
    // in a container without netlink the open is refused with an error, never a panic
    match crate::uevent::UeventSocket::open() {
        Ok(socket) => assert!(socket
            .next(Some(std::time::Duration::from_millis(10)))
            .is_ok()),
        Err(errno) => assert!(errno.0 > 0),
    }
}
