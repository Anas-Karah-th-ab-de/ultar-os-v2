//! The device client against the real server: signed-out store and updates, premium and gated
//! content, error mapping to message keys, and the signature gate at installation.

use ultar_cloud::messages;
use ultar_cloud::{AppAccess, CatalogQuery, Client, CloudError, Subject};
use ultar_cloud_e2e::Stack;
use ultar_i18n::{Lang, Translator};
use ultraos_server::admin;
use ultraos_server::entitlements::EntitlementKind;
use ultraos_server::ratelimit::{BucketConfig, LimiterConfig};

fn english() -> Translator {
    let mut tr = Translator::empty(Lang::En);
    messages::register(&mut tr).unwrap();
    tr
}

fn install(stack: &Stack, client: &Client, id: &str) -> Result<ultar_pkg::Manifest, CloudError> {
    client.install_app_at(id, &stack.work(), &stack.trusted(), &stack.root())
}

#[test]
fn signed_out_device_browses_and_installs_a_free_app() {
    let s = Stack::start();
    admin::set_publisher(&s.state.db, "KM Code Craft", true).unwrap();
    s.publish_signed("com.kmcc.notes", "1.0.0", false);
    s.publish_signed("com.kmcc.pro", "2.0.0", true);
    let device = s.anonymous();
    assert!(!device.is_signed_in());

    let page = device.catalog(&CatalogQuery::default()).unwrap();
    assert_eq!(page.total, 2);
    let found = device
        .catalog(&CatalogQuery {
            text: "notes".into(),
            ..CatalogQuery::default()
        })
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].id, "com.kmcc.notes");
    assert!(found.items[0].publisher.verified);
    let by_type = device
        .catalog(&CatalogQuery {
            text: ".txt".into(),
            ..CatalogQuery::default()
        })
        .unwrap();
    assert_eq!(by_type.total, 2);
    assert_eq!(device.categories().unwrap()[0].id, "office");
    assert_eq!(device.store_list().unwrap().len(), 2);

    let details = device.app("com.kmcc.notes").unwrap();
    assert_eq!(details.permissions, vec!["network"]);
    assert_eq!(details.versions.len(), 1);
    assert_eq!(AppAccess::of(&details.summary, None), AppAccess::Free);
    let pro = device.app("com.kmcc.pro").unwrap();
    assert_eq!(
        AppAccess::of(&pro.summary, None),
        AppAccess::PremiumSignInRequired
    );

    let manifest = install(&s, &device, "com.kmcc.notes").unwrap();
    assert_eq!(
        (manifest.id.as_str(), manifest.version.as_str()),
        ("com.kmcc.notes", "1.0.0")
    );
    let installed = s.root().join("data/ultar/apps/com.kmcc.notes/app.txt");
    assert_eq!(
        std::fs::read_to_string(installed).unwrap(),
        "com.kmcc.notes 1.0.0"
    );
    assert!(s.no_leftover_downloads());
}

#[test]
fn premium_app_states_map_to_typed_errors_and_message_keys() {
    let s = Stack::start();
    s.publish_signed("com.kmcc.pro", "2.0.0", true);
    let tr = english();

    let err = install(&s, &s.anonymous(), "com.kmcc.pro").unwrap_err();
    assert!(matches!(err, CloudError::AuthRequired), "{err}");
    assert_eq!(
        err.message_key(Subject::App),
        "cloud.store.sign_in_required"
    );
    assert_eq!(
        messages::describe(&tr, &err, Subject::App),
        "Sign in to get this app."
    );

    let anna = s.signed_in("anna");
    assert!(anna.is_signed_in());
    let err = install(&s, &anna, "com.kmcc.pro").unwrap_err();
    assert!(matches!(err, CloudError::NotEntitled), "{err}");
    assert_eq!(
        messages::describe(&tr, &err, Subject::App),
        "This app is not included in your access."
    );
    let pro = anna.app("com.kmcc.pro").unwrap();
    let mine = anna.entitlements().unwrap();
    assert_eq!(
        AppAccess::of(&pro.summary, Some(&mine)),
        AppAccess::PremiumNotIncluded
    );
    assert!(s.nothing_installed() && s.no_leftover_downloads());

    admin::grant(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap();
    let mine = anna.entitlements().unwrap();
    assert_eq!(
        AppAccess::of(&pro.summary, Some(&mine)),
        AppAccess::Unlocked
    );
    assert_eq!(
        install(&s, &anna, "com.kmcc.pro").unwrap().id,
        "com.kmcc.pro"
    );

    admin::revoke(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap();
    let err = install(&s, &anna, "com.kmcc.pro").unwrap_err();
    assert!(matches!(err, CloudError::NotEntitled));

    let err = s.anonymous().app("no.such.app").unwrap_err();
    assert!(matches!(err, CloudError::NotFound));
    assert_eq!(err.message_key(Subject::App), "cloud.store.not_found");
    assert!(matches!(
        s.anonymous().app("../../etc/passwd"),
        Err(CloudError::NotFound)
    ));
}

#[test]
fn a_dishonest_server_cannot_get_anything_installed() {
    let s = Stack::start();
    let (attacker_key, _) = ultar_pkg::keygen();

    // 1. Package signed with a key the device does not trust.
    let forged = s.build_package("com.kmcc.forged", "1.0.0", &attacker_key);
    s.publish("com.kmcc.forged", "1.0.0", false, &forged);
    let err = install(&s, &s.anonymous(), "com.kmcc.forged").unwrap_err();
    assert!(matches!(err, CloudError::Package(_)), "{err}");
    assert_eq!(
        err.message_key(Subject::App),
        "cloud.error.package_rejected"
    );

    // 2. Not a package at all.
    let junk = s.dir.path().join("junk.uap");
    std::fs::write(&junk, b"definitely not a package").unwrap();
    s.publish("com.kmcc.junk", "1.0.0", false, &junk);
    assert!(matches!(
        install(&s, &s.anonymous(), "com.kmcc.junk"),
        Err(CloudError::Package(_))
    ));

    // 3. A genuine, correctly signed package of ANOTHER app offered under the requested id.
    let other = s.build_package("com.kmcc.other", "1.0.0", &s.publisher_key);
    s.publish("com.kmcc.target", "1.0.0", false, &other);
    let err = install(&s, &s.anonymous(), "com.kmcc.target").unwrap_err();
    assert!(matches!(err, CloudError::Package(_)), "{err}");

    // 4. File changed on the server after publication (same length): caught as damaged.
    let sha = s.publish_signed("com.kmcc.good", "1.0.0", false);
    let stored = s.dir.path().join("files").join(&sha);
    let mut bytes = std::fs::read(&stored).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&stored, &bytes).unwrap();
    let err = install(&s, &s.anonymous(), "com.kmcc.good").unwrap_err();
    assert!(matches!(err, CloudError::Integrity), "{err}");
    assert_eq!(err.message_key(Subject::App), "cloud.error.integrity");

    assert!(
        s.nothing_installed(),
        "none of the four attempts wrote to the device"
    );
    assert!(s.no_leftover_downloads());
}

#[test]
fn updates_of_open_channels_work_signed_out() {
    let s = Stack::start();
    let arch = std::env::consts::ARCH;
    s.publish_release("stable", "0.7.0", b"stable-image");
    let device = s.anonymous();

    let release = device
        .latest_update("stable", arch, "desktop")
        .unwrap()
        .unwrap();
    assert_eq!((release.version.as_str(), release.size), ("0.7.0", 12));
    assert_eq!(release.roothash, "ab12");
    let image = s.dir.path().join("update.img");
    device.download_update(&release, &image).unwrap();
    assert_eq!(std::fs::read(&image).unwrap(), b"stable-image");
    assert_eq!(
        device.latest_update("beta", arch, "desktop").unwrap(),
        None,
        "no release yet"
    );
    assert_eq!(
        device
            .latest_update("stable", "riscv64", "desktop")
            .unwrap(),
        None
    );

    let err = device
        .latest_update("no-such-channel", arch, "desktop")
        .unwrap_err();
    assert!(matches!(err, CloudError::NotFound));
    assert_eq!(
        err.message_key(Subject::UpdateChannel),
        "cloud.update.not_found"
    );
}

#[test]
fn gated_update_channels_need_an_account_with_access() {
    let s = Stack::start();
    let tr = english();
    let arch = std::env::consts::ARCH;
    s.publish_release("preview", "0.8.0-pre", b"preview-image");
    let device = s.anonymous();

    let err = device
        .latest_update("preview", arch, "desktop")
        .unwrap_err();
    assert!(matches!(err, CloudError::AuthRequired));
    let text = messages::describe(&tr, &err, Subject::UpdateChannel);
    assert_eq!(text, "Sign in to use this update channel.");

    let anna = s.signed_in("anna");
    let err = anna.latest_update("preview", arch, "desktop").unwrap_err();
    assert!(matches!(err, CloudError::NotEntitled));
    assert_eq!(
        err.message_key(Subject::UpdateChannel),
        "cloud.update.not_included"
    );

    admin::grant(&s.state.db, "anna", EntitlementKind::Channel, "preview").unwrap();
    let preview = anna
        .latest_update("preview", arch, "desktop")
        .unwrap()
        .unwrap();
    assert_eq!(preview.version, "0.8.0-pre");
    let image = s.dir.path().join("update.img");
    anna.download_update(&preview, &image).unwrap();
    assert_eq!(std::fs::read(&image).unwrap(), b"preview-image");
    // The same release is closed to a signed-out device even with the URL in hand.
    let stolen = s.dir.path().join("stolen.img");
    let err = device.download_update(&preview, &stolen).unwrap_err();
    assert!(matches!(err, CloudError::AuthRequired));
    assert!(!stolen.exists());
}

#[test]
fn rate_limit_is_reported_with_the_wait_and_clears() {
    let tight = BucketConfig {
        capacity: 2,
        refill_per_minute: 6,
    };
    let roomy = BucketConfig {
        capacity: 1000,
        refill_per_minute: 1000,
    };
    let s = Stack::start_with(LimiterConfig {
        general: tight,
        download: roomy,
        auth: roomy,
        max_entries: 100,
    });
    let device = s.anonymous();
    device.categories().unwrap();
    device.categories().unwrap();
    let err = device.categories().unwrap_err();
    assert!(
        matches!(
            err,
            CloudError::RateLimited {
                retry_after_secs: 10
            }
        ),
        "{err}"
    );
    let text = messages::describe(&english(), &err, Subject::App);
    assert_eq!(text, "Too many requests. Try again in 10 seconds.");
    s.clock.advance_millis(10_000);
    device.categories().unwrap();
}

#[test]
fn account_calls_need_a_token_and_keep_working() {
    let s = Stack::start();
    let device = s.anonymous();
    assert!(matches!(
        device.entitlements(),
        Err(CloudError::NotSignedIn)
    ));
    let home = s.device_home("phone");
    assert!(matches!(
        home.register_number(&device, "", None),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(device.vpn_peer("x"), Err(CloudError::NotSignedIn)));

    let anna = s.signed_in("anna");
    let number = home.register_number(&anna, "", None).unwrap();
    assert!(number.starts_with("UOS-"));
    assert_eq!(home.register_number(&anna, "", None).unwrap(), number);
    let peer = anna
        .vpn_peer("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
        .unwrap();
    assert_eq!(
        (peer.address.as_str(), peer.server_pubkey.as_str()),
        ("10.80.0.2/32", "server-pubkey")
    );
    assert!(matches!(
        anna.vpn_peer("bad"),
        Err(CloudError::InvalidRequest)
    ));

    let token = device.login("anna", "password-123").unwrap();
    assert!(Client::new(&s.base(), Some(token))
        .entitlements()
        .unwrap()
        .apps
        .is_empty());
    assert!(matches!(
        device.login("anna", "wrong-password"),
        Err(CloudError::InvalidCredentials)
    ));
    assert!(matches!(
        device.register("anna", "password-123"),
        Err(CloudError::UsernameTaken)
    ));
    assert!(matches!(
        device.register("x", "short"),
        Err(CloudError::InvalidRequest)
    ));

    // An invalid stored token behaves like "sign in again", not like a crash.
    let stale = Client::new(&s.base(), Some("stale-token".into()));
    assert!(matches!(
        stale.entitlements(),
        Err(CloudError::AuthRequired)
    ));
    assert!(
        stale.catalog(&CatalogQuery::default()).is_ok(),
        "public calls ignore a stale token"
    );
}
