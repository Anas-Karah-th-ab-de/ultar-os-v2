//! Getting a bundle onto the device: asking a source, verifying the header, downloading or
//! copying into the staging area. Further `impl` block of [`Engine`].

use crate::bootctl::BootControl;
use crate::bundle::{Header, MAX_HEADER};
use crate::engine::{Engine, Offer};
use crate::error::{NetworkError, UpdateError};
use crate::history::HistoryKind;
use crate::journal::Journal;
use crate::source::{bundles_in, FileSource, Query, Remote, UpdateSource};
use crate::stage::{FetchEnv, NoPacing, Pacer, StagedInfo};
use crate::verify;
use crate::version::Version;
use std::path::Path;

impl<B: BootControl> Engine<B> {
    /// Asks `source` for the newest release of `channel` and verifies its header.
    /// `Ok(None)`: the source has nothing, or what it has is the running version.
    pub fn check(
        &mut self,
        source: &mut dyn UpdateSource,
        channel: &str,
    ) -> Result<Option<Offer>, UpdateError> {
        let device = self.config.device.clone();
        let query = Query {
            channel,
            arch: &device.arch,
            edition: &device.edition,
        };
        let Some(remote) = source.latest(&query)? else {
            return Ok(None);
        };
        match self.offer_from(source, remote, Some(channel), false) {
            Err(UpdateError::UpToDate) => Ok(None),
            other => other.map(Some),
        }
    }

    fn offer_from(
        &mut self,
        source: &mut dyn UpdateSource,
        remote: Remote,
        channel: Option<&str>,
        allow_downgrade: bool,
    ) -> Result<Offer, UpdateError> {
        let (start, mut reader) = source.open(&remote, 0, Some(MAX_HEADER as u64))?;
        if start != 0 {
            return Err(UpdateError::Network(NetworkError::Protocol));
        }
        let header = Header::read(&mut *reader)?;
        drop(reader);
        let verified = self.inspect(&header, channel, allow_downgrade)?;
        Ok(Offer {
            channel: channel.map(str::to_string),
            remote,
            header,
            verified,
        })
    }

    /// Collects the bundle of `offer` into the staging area (resuming an earlier attempt)
    /// after checking the free space. Returns when the bundle is complete and verified.
    pub fn download(
        &mut self,
        source: &mut dyn UpdateSource,
        offer: &Offer,
        pacer: &mut dyn Pacer,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<StagedInfo, UpdateError> {
        self.fetch(source, offer, pacer, progress, false)
    }

    fn fetch(
        &mut self,
        source: &mut dyn UpdateSource,
        offer: &Offer,
        pacer: &mut dyn Pacer,
        progress: &mut dyn FnMut(u64, u64),
        allow_downgrade: bool,
    ) -> Result<StagedInfo, UpdateError> {
        if matches!(self.journal()?, Journal::Trial { .. }) {
            return Err(UpdateError::Busy);
        }
        let total = offer.bundle_size();
        let complete = self
            .staging
            .staged()?
            .is_some_and(|s| s.digest == offer.verified.digest());
        if !complete {
            let missing = total.saturating_sub(self.staging.partial_bytes());
            let needed = missing.saturating_add(self.config.reserve_bytes);
            let free = self.env.space.free_bytes(&self.config.state_dir)?;
            if free < needed {
                return Err(UpdateError::NoSpace { needed, free });
            }
        }
        let control = self.env.control.clone();
        let mut env = FetchEnv {
            control: &*control,
            pacer,
            progress,
            allow_downgrade,
            channel: offer.channel.as_deref(),
        };
        let staged = self.staging.fetch(
            &offer.header,
            &offer.verified,
            source,
            &offer.remote,
            &mut env,
        )?;
        if !complete {
            let version = offer.verified.manifest().version;
            self.log(HistoryKind::Staged, Some(version), None);
        }
        Ok(staged)
    }

    /// Offline update: looks at the bundle file or folder `location`, verifies every
    /// candidate header, picks the newest acceptable release and copies it into the staging
    /// area with the same checks as a download. When no candidate is acceptable the error of
    /// the first one is returned ([`UpdateError::NoUpdate`] for an empty folder).
    pub fn stage_offline(
        &mut self,
        location: &Path,
        allow_downgrade: bool,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Offer, UpdateError> {
        let mut best: Option<(Offer, FileSource)> = None;
        let mut first_error = None;
        for path in bundles_in(location)? {
            let mut source = FileSource::new(&path);
            let remote = Remote {
                locator: path.to_string_lossy().into_owned(),
                size_hint: 0,
            };
            match self.offer_from(&mut source, remote, None, allow_downgrade) {
                Ok(offer) => {
                    let newer = best.as_ref().is_none_or(|(b, _)| {
                        offer.verified.manifest().version > b.verified.manifest().version
                    });
                    if newer {
                        best = Some((offer, source));
                    }
                }
                Err(e) => first_error = first_error.or(Some(e)),
            }
        }
        let Some((offer, mut source)) = best else {
            return Err(first_error.unwrap_or(UpdateError::NoUpdate));
        };
        let mut pacer = NoPacing;
        self.fetch(&mut source, &offer, &mut pacer, progress, allow_downgrade)?;
        Ok(offer)
    }

    /// The version of the complete bundle in the staging area, if there is one and its
    /// manifest still authenticates.
    pub fn staged_version(&self) -> Option<Version> {
        let (mut file, _) = self.staging.open_staged().ok().flatten()?;
        let header = Header::read(&mut file).ok()?;
        let authentic = verify::authenticate(&header.signed_manifest, self.trust.keys()).ok()?;
        Some(authentic.manifest().version)
    }
}
