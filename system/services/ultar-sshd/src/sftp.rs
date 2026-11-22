//! SFTP-Teil (Dateien hin und her, `scp`/`sftp` vom PC; Windows-`scp` spricht SFTP).

use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, SftpHandle, Status, StatusCode, Version};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

enum Offen {
    Datei(fs::File),
    Ordner(Option<Vec<File>>),
}

#[derive(Default)]
pub struct Sitzung {
    offen: HashMap<String, Offen>,
    zaehler: u64,
    home: String,
}

impl Sitzung {
    pub fn neu(home: &str) -> Self {
        Self { home: home.into(), ..Default::default() }
    }
    fn pfad(&self, p: &str) -> PathBuf {
        if p.is_empty() || p == "." {
            PathBuf::from(&self.home)
        } else if p.starts_with('/') {
            PathBuf::from(p)
        } else {
            Path::new(&self.home).join(p)
        }
    }
    fn neuer_griff(&mut self, o: Offen) -> String {
        self.zaehler += 1;
        let h = format!("h{}", self.zaehler);
        self.offen.insert(h.clone(), o);
        h
    }
}

fn fehler(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn ok(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "de".into() }
}

fn h(x: &SftpHandle) -> String {
    String::from_utf8_lossy(x.as_bytes()).into_owned()
}

/// Pfad ohne . und .. (rein textuell – wie realpath ohne Dateisystemzugriff)
fn normal(p: &Path) -> String {
    let mut v: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                v.pop();
            }
            std::path::Component::Normal(s) => v.push(s.to_string_lossy().into()),
            _ => {}
        }
    }
    format!("/{}", v.join("/"))
}

impl russh_sftp::server::Handler for Sitzung {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _v: u32, _e: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(&mut self, id: u32, name: String, fl: OpenFlags, _a: FileAttributes) -> Result<Handle, Self::Error> {
        let mut o = fs::OpenOptions::new();
        o.read(fl.contains(OpenFlags::READ))
            .write(fl.contains(OpenFlags::WRITE) || fl.contains(OpenFlags::APPEND))
            .append(fl.contains(OpenFlags::APPEND))
            .truncate(fl.contains(OpenFlags::TRUNCATE));
        if fl.contains(OpenFlags::CREATE) {
            if fl.contains(OpenFlags::EXCLUDE) {
                o.create_new(true);
            } else {
                o.create(true);
            }
        }
        let f = o.open(self.pfad(&name)).map_err(fehler)?;
        Ok(Handle { id, handle: self.neuer_griff(Offen::Datei(f)).into() })
    }

    async fn close(&mut self, id: u32, handle: SftpHandle) -> Result<Status, Self::Error> {
        if let Some(Offen::Datei(mut f)) = self.offen.remove(&h(&handle)) {
            let _ = f.flush();
        }
        Ok(ok(id))
    }

    async fn read(&mut self, id: u32, handle: SftpHandle, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let Some(Offen::Datei(f)) = self.offen.get_mut(&h(&handle)) else { return Err(StatusCode::Failure) };
        f.seek(SeekFrom::Start(offset)).map_err(fehler)?;
        let mut b = vec![0u8; len.min(256 * 1024) as usize];
        let n = f.read(&mut b).map_err(fehler)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        b.truncate(n);
        Ok(Data { id, data: b })
    }

    async fn write(&mut self, id: u32, handle: SftpHandle, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let Some(Offen::Datei(f)) = self.offen.get_mut(&h(&handle)) else { return Err(StatusCode::Failure) };
        f.seek(SeekFrom::Start(offset)).map_err(fehler)?;
        f.write_all(&data).map_err(fehler)?;
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let m = fs::symlink_metadata(self.pfad(&path)).map_err(fehler)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&m) })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let m = fs::metadata(self.pfad(&path)).map_err(fehler)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&m) })
    }

    async fn fstat(&mut self, id: u32, handle: SftpHandle) -> Result<Attrs, Self::Error> {
        match self.offen.get(&h(&handle)) {
            Some(Offen::Datei(f)) => Ok(Attrs { id, attrs: FileAttributes::from(&f.metadata().map_err(fehler)?) }),
            Some(Offen::Ordner(_)) => {
                let mut a = FileAttributes::empty();
                a.set_dir(true);
                Ok(Attrs { id, attrs: a })
            }
            None => Err(StatusCode::Failure),
        }
    }

    async fn setstat(&mut self, id: u32, path: String, a: FileAttributes) -> Result<Status, Self::Error> {
        if let Some(n) = a.size {
            let f = fs::OpenOptions::new().write(true).open(self.pfad(&path)).map_err(fehler)?;
            f.set_len(n).map_err(fehler)?;
        }
        if let Some(p) = a.permissions {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.pfad(&path), fs::Permissions::from_mode(p & 0o7777));
        }
        Ok(ok(id))
    }

    async fn fsetstat(&mut self, id: u32, handle: SftpHandle, a: FileAttributes) -> Result<Status, Self::Error> {
        if let (Some(n), Some(Offen::Datei(f))) = (a.size, self.offen.get(&h(&handle))) {
            f.set_len(n).map_err(fehler)?;
        }
        Ok(ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let p = self.pfad(&path);
        let mut v = vec![File::new(".", FileAttributes::from(&fs::metadata(&p).map_err(fehler)?))];
        for e in fs::read_dir(&p).map_err(fehler)?.flatten() {
            let a = e.metadata().map(|m| FileAttributes::from(&m)).unwrap_or_default();
            v.push(File::new(e.file_name().to_string_lossy().into_owned(), a));
        }
        Ok(Handle { id, handle: self.neuer_griff(Offen::Ordner(Some(v))).into() })
    }

    async fn readdir(&mut self, id: u32, handle: SftpHandle) -> Result<Name, Self::Error> {
        match self.offen.get_mut(&h(&handle)) {
            Some(Offen::Ordner(v)) => match v.take() {
                Some(files) => Ok(Name { id, files }),
                None => Err(StatusCode::Eof),
            },
            _ => Err(StatusCode::Failure),
        }
    }

    async fn remove(&mut self, id: u32, f: String) -> Result<Status, Self::Error> {
        fs::remove_file(self.pfad(&f)).map_err(fehler)?;
        Ok(ok(id))
    }

    async fn mkdir(&mut self, id: u32, p: String, _a: FileAttributes) -> Result<Status, Self::Error> {
        fs::create_dir(self.pfad(&p)).map_err(fehler)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, p: String) -> Result<Status, Self::Error> {
        fs::remove_dir(self.pfad(&p)).map_err(fehler)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, p: String) -> Result<Name, Self::Error> {
        let pf = self.pfad(&p);
        let r = fs::canonicalize(&pf).map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|_| normal(&pf));
        Ok(Name { id, files: vec![File::dummy(r)] })
    }

    async fn rename(&mut self, id: u32, a: String, b: String) -> Result<Status, Self::Error> {
        fs::rename(self.pfad(&a), self.pfad(&b)).map_err(fehler)?;
        Ok(ok(id))
    }

    async fn readlink(&mut self, id: u32, p: String) -> Result<Name, Self::Error> {
        let t = fs::read_link(self.pfad(&p)).map_err(fehler)?;
        Ok(Name { id, files: vec![File::dummy(t.to_string_lossy().into_owned())] })
    }

    async fn symlink(&mut self, id: u32, link: String, target: String) -> Result<Status, Self::Error> {
        std::os::unix::fs::symlink(target, self.pfad(&link)).map_err(fehler)?;
        Ok(ok(id))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn normal_pfad() {
        assert_eq!(super::normal(std::path::Path::new("/a/b/../c/./d")), "/a/c/d");
        assert_eq!(super::normal(std::path::Path::new("/..")), "/");
    }
}
