//! The service definition shipped in `rootfs/etc/ultar/services/logd.toml`
//! must be a command line the daemon accepts, with bounds that are valid.

use std::path::Path;
use ultar_logd::daemon::{parse, DaemonCommand};
use ultar_logd::store::FsyncPolicy;

#[test]
fn the_shipped_definition_parses_with_valid_bounds() {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/services/logd.toml");
    let table: toml::Table = std::fs::read_to_string(file).unwrap().parse().unwrap();
    let args: Vec<String> = table["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect();
    let DaemonCommand::Run(args) = parse(args).unwrap() else {
        panic!("the definition must run the daemon");
    };
    assert_eq!(args.dir, Path::new("/data/ultar/log"));
    // One socket per writing service (its group) plus the output channels of init and the launcher.
    const WRITERS: [u32; 21] = [
        801, 803, 804, 805, 813, 817, 818, 819, 820, 821, 822, 823, 824, 825, 826, 827, 828, 833,
        834, 835, 837,
    ];
    // Root-only writers (mode 0600, no group): the update service.
    const ROOT_WRITERS: [&str; 1] = ["system.updated"];
    assert_eq!(args.listeners.len(), WRITERS.len() + ROOT_WRITERS.len() + 2);
    let (output, writers): (Vec<_>, Vec<_>) = args.listeners.iter().partition(|l| l.output);
    let (root, writers): (Vec<_>, Vec<_>) = writers
        .into_iter()
        .partition(|l| l.name.as_deref().is_some_and(|n| ROOT_WRITERS.contains(&n)));
    assert_eq!(root.len(), ROOT_WRITERS.len());
    for listener in &root {
        assert_eq!((listener.mode, listener.group), (Some(0o600), None));
    }
    for listener in &writers {
        // Fixed names (one socket per writer), open to the writer's group only.
        assert!(listener
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("system.")));
        assert_eq!(listener.mode, Some(0o660));
        assert!(listener.group.is_some_and(|gid| WRITERS.contains(&gid)));
    }
    // The output channel of ultar-init: root only; the app launcher's
    // (apps' output, stored as app-<id>): its group only.
    assert_eq!(output.len(), 2);
    assert_eq!((output[0].mode, output[0].group), (Some(0o660), Some(805)));
    assert_eq!((output[1].mode, output[1].group), (Some(0o600), None));
    assert!(args.config.store.is_valid());
    assert_eq!(args.config.store.segment_bytes, 1 << 20);
    assert_eq!(args.config.store.total_bytes, 16 << 20);
    assert_eq!(args.config.store.max_age_ms, 30 * 24 * 3600 * 1000);
    assert!(matches!(args.config.store.fsync, FsyncPolicy::Batch(_)));
    assert_eq!(
        args.bus.as_deref(),
        Some(Path::new("/run/ultar/bus/logd.sock"))
    );
    // Started as its own user by ultar-init: root is neither needed nor declared.
    assert!(args.policy.is_some() && !args.allow_all && !args.allow_root);
    assert!(
        args.chain_key.is_none(),
        "no key file is shipped; see INTEGRATION.md"
    );
}
