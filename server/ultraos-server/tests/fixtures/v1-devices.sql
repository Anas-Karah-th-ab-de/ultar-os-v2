-- Database written by ultraos-server at schema 1 (the repository code before the device-identity change).
-- Produced by running that server binary and calling its HTTP API; see tests/device_migration.rs.
BEGIN TRANSACTION;
CREATE TABLE accounts (
    id         INTEGER PRIMARY KEY,
    username   TEXT UNIQUE NOT NULL,
    pw_hash    TEXT NOT NULL,          -- Argon2id (PHC-String)
    created    INTEGER NOT NULL
);
INSERT INTO "accounts" VALUES(1,'fixture-anna','$argon2id$v=19$m=19456,t=2,p=1$Rqv58kp6ZBZ9u7yJIQtt5A$BKiRVispuNT7p78AkyhbmWJ+a7yACHJ2+8MPhhfllC8',1795394631);
INSERT INTO "accounts" VALUES(2,'fixture-mallory','$argon2id$v=19$m=19456,t=2,p=1$cC4QOCf0MbWmvbFvgJrf8Q$YQhPnp2F5Le0M41UFXpfV2djxIslT8H9st//054zsa0',1795394632);
INSERT INTO "accounts" VALUES(3,'fixture-bert','$argon2id$v=19$m=19456,t=2,p=1$sR8LjTg5CtD/O3ifAtNVVw$Txbm8txiaUWlVhHuo7mci/p45Uvi5sQIupBY5avfyO8',1795394632);
CREATE TABLE audit (
    id      INTEGER PRIMARY KEY,
    ts      INTEGER NOT NULL,
    who     TEXT,
    action  TEXT NOT NULL,
    detail  TEXT
);
CREATE TABLE devices (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id  TEXT UNIQUE NOT NULL,   -- Hash(Seriennr + Gerätepubkey)
    pubkey     TEXT NOT NULL,          -- ed25519 (base64) des Geräts
    created    INTEGER NOT NULL
);
INSERT INTO "devices" VALUES(1,1,'a3f497b589a474e9','cHVia2V5',1795394632);
INSERT INTO "devices" VALUES(2,1,'a1b04767f0484668','',1795394632);
INSERT INTO "devices" VALUES(3,2,'7af8c5b4c52e91a3','',1795394632);
CREATE TABLE entitlements (
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('app','channel')),
    item       TEXT NOT NULL,
    granted    INTEGER NOT NULL,
    PRIMARY KEY (account_id, kind, item)
);
CREATE TABLE numbers (
    number     TEXT PRIMARY KEY,       -- UOS-XXXX-XXXX
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id  TEXT NOT NULL REFERENCES devices(device_id) ON DELETE CASCADE,
    created    INTEGER NOT NULL
);
INSERT INTO "numbers" VALUES('UOS-D728-EDAD',1,'a3f497b589a474e9',1795394632);
INSERT INTO "numbers" VALUES('UOS-857F-8E06',1,'a1b04767f0484668',1795394632);
INSERT INTO "numbers" VALUES('UOS-8066-6C00',2,'7af8c5b4c52e91a3',1795394632);
CREATE TABLE publishers (
    name       TEXT PRIMARY KEY,
    verified   INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE "releases" (
    id         INTEGER PRIMARY KEY,
    channel    TEXT NOT NULL,
    version    TEXT NOT NULL,
    arch       TEXT NOT NULL DEFAULT 'any',
    edition    TEXT NOT NULL DEFAULT 'any',
    url        TEXT NOT NULL DEFAULT '',   -- external location of rows from schema 0
    roothash   TEXT NOT NULL,
    sig        TEXT NOT NULL,
    size       INTEGER NOT NULL DEFAULT 0,
    sha256     TEXT NOT NULL DEFAULT '',
    stored     INTEGER NOT NULL DEFAULT 0,
    created    INTEGER NOT NULL,
    UNIQUE (channel, version, arch, edition)
);
CREATE TABLE store_apps (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    version    TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    sig        TEXT NOT NULL,          -- ed25519-Signatur des Pakets (base64)
    url        TEXT NOT NULL
, description TEXT NOT NULL DEFAULT '', category TEXT NOT NULL DEFAULT 'other', publisher TEXT NOT NULL DEFAULT '', premium INTEGER NOT NULL DEFAULT 0, permissions TEXT NOT NULL DEFAULT '[]', installed_size INTEGER NOT NULL DEFAULT 0, search_text TEXT NOT NULL DEFAULT '', stored INTEGER NOT NULL DEFAULT 0);
CREATE TABLE store_filetypes (
    app_id     TEXT NOT NULL REFERENCES store_apps(id) ON DELETE CASCADE,
    ext        TEXT NOT NULL,              -- lower case, without dot
    PRIMARY KEY (app_id, ext)
);
CREATE TABLE store_versions (
    app_id     TEXT NOT NULL REFERENCES store_apps(id) ON DELETE CASCADE,
    version    TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    sig        TEXT NOT NULL DEFAULT '',
    url        TEXT NOT NULL DEFAULT '',   -- external location of rows from schema 0
    stored     INTEGER NOT NULL DEFAULT 0, -- 1 = file is in the server's file store
    changelog  TEXT NOT NULL DEFAULT '',
    created    INTEGER NOT NULL,
    PRIMARY KEY (app_id, version)
);
CREATE TABLE update_channels (
    name       TEXT PRIMARY KEY,
    gated      INTEGER NOT NULL DEFAULT 0  -- 1 = needs token + entitlement
);
INSERT INTO "update_channels" VALUES('stable',0);
INSERT INTO "update_channels" VALUES('beta',0);
INSERT INTO "update_channels" VALUES('preview',1);
INSERT INTO "update_channels" VALUES('lts',1);
CREATE TABLE verbund_geraete (
    pk         TEXT PRIMARY KEY,       -- Noise-X25519-Schlüssel (hex) des Geräts
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    ausgabe    TEXT NOT NULL,          -- handy | laptop | server | platine
    zuletzt    INTEGER NOT NULL
);
CREATE TABLE wg_peers (
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    pubkey     TEXT NOT NULL,          -- WireGuard-Pubkey des Geräts
    address    TEXT NOT NULL,          -- zugewiesene interne IP (10.80.x.y/32)
    created    INTEGER NOT NULL,
    PRIMARY KEY (account_id, pubkey)
);
CREATE INDEX store_filetypes_ext ON store_filetypes(ext);
CREATE INDEX store_apps_category ON store_apps(category);
CREATE INDEX releases_lookup ON releases(channel, created);
COMMIT;
PRAGMA user_version=1;
