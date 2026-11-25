//! Archives for the Files tab: list and extract `.zip`, `.7z` and `.rar`, and create `.zip` files with AES-256.
//!
//! Zip is read and written with the `zip` crate (AES only for new archives; ZipCrypto is not written). 7z is read
//! with `sevenz-rust2` (pure Rust). RAR is read with `unrar` (the unrar library, which can extract but not create
//! RAR files). No archive here is ever created in a format other than zip.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use zip::read::ZipArchive;
use zip::write::{FileOptions, SimpleFileOptions, ZipWriter};
use zip::{AesMode, CompressionMethod};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    SevenZ,
    Rar,
}

/// The archive format for a path, from its extension.
pub fn format_of(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "zip" => Some(Format::Zip),
        "7z" => Some(Format::SevenZ),
        "rar" => Some(Format::Rar),
        _ => None,
    }
}

/// One entry of an archive. `path` uses `/` between folders and is relative to the archive's root.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub encrypted: bool,
}

#[derive(Debug)]
pub enum ArchiveError {
    /// The archive (or this entry) is encrypted and no password was given.
    PasswordRequired,
    /// The password was given and is wrong.
    WrongPassword,
    Failed(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PasswordRequired => write!(f, "this archive needs a password"),
            Self::WrongPassword => write!(f, "the password is wrong"),
            Self::Failed(text) => write!(f, "{text}"),
        }
    }
}

impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        Self::Failed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ArchiveError>;

/// Lists the entries of an archive. Zip and RAR list their names without a password unless the RAR headers are
/// encrypted; 7z may need the password to read its listing.
pub fn list(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    match format_of(path) {
        Some(Format::Zip) => list_zip(path),
        Some(Format::SevenZ) => list_7z(path, password),
        Some(Format::Rar) => list_rar(path, password),
        None => Err(ArchiveError::Failed("not an archive".into())),
    }
}

fn list_zip(path: &Path) -> Result<Vec<Entry>> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))
        .map_err(|e| ArchiveError::Failed(e.to_string()))?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let file = archive
            .by_index_raw(i)
            .map_err(|e| ArchiveError::Failed(e.to_string()))?;
        let name = file
            .name()
            .map_err(|e| ArchiveError::Failed(e.to_string()))?
            .trim_end_matches('/')
            .to_string();
        out.push(Entry {
            path: name,
            is_dir: file.is_dir(),
            size: file.size(),
            encrypted: file.encrypted(),
        });
    }
    Ok(out)
}

fn list_7z(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let pw = sevenz_password(password);
    let mut reader = sevenz_rust2::ArchiveReader::open(path, pw).map_err(map_7z)?;
    let mut out = Vec::new();
    reader
        .for_each_entries(|entry, _stream| {
            out.push(Entry {
                path: entry.name.trim_end_matches('/').to_string(),
                is_dir: entry.is_directory,
                size: entry.size,
                encrypted: false,
            });
            Ok(true)
        })
        .map_err(map_7z)?;
    Ok(out)
}

fn list_rar(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let archive = match password {
        Some(pw) => unrar::Archive::with_password(path, pw),
        None => unrar::Archive::new(path),
    };
    let listing = archive
        .open_for_listing()
        .map_err(|e| map_rar(&e.to_string()))?;
    let mut out = Vec::new();
    for header in listing {
        let header = header.map_err(|e| map_rar(&e.to_string()))?;
        out.push(Entry {
            path: header
                .filename
                .to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .replace('\\', "/"),
            is_dir: header.is_directory(),
            size: header.unpacked_size,
            encrypted: header.is_encrypted(),
        });
    }
    Ok(out)
}

/// Extracts every entry of an archive into `dest` (created if needed). Returns the number of files written.
pub fn extract_all(path: &Path, dest: &Path, password: Option<&str>) -> Result<usize> {
    fs::create_dir_all(dest)?;
    match format_of(path) {
        Some(Format::Zip) => extract_zip(path, dest, password),
        Some(Format::SevenZ) => extract_7z(path, dest, password),
        Some(Format::Rar) => extract_rar(path, dest, password),
        None => Err(ArchiveError::Failed("not an archive".into())),
    }
}

fn extract_zip(path: &Path, dest: &Path, password: Option<&str>) -> Result<usize> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))
        .map_err(|e| ArchiveError::Failed(e.to_string()))?;
    let mut files = 0;
    for i in 0..archive.len() {
        let encrypted = archive
            .by_index_raw(i)
            .map_err(|e| ArchiveError::Failed(e.to_string()))?
            .encrypted();
        let mut file = match (encrypted, password) {
            (true, None) => return Err(ArchiveError::PasswordRequired),
            (true, Some(pw)) => archive.by_index_decrypt(i, pw.as_bytes()).map_err(map_zip)?,
            (false, _) => archive.by_index(i).map_err(map_zip)?,
        };
        // enclosed_name refuses names that climb out of the folder (../), so extraction stays inside dest.
        let Some(rel) = file.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if file.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut sink = File::create(&out)?;
            io::copy(&mut file, &mut sink).map_err(map_zip_io)?;
            files += 1;
        }
    }
    Ok(files)
}

fn extract_7z(path: &Path, dest: &Path, password: Option<&str>) -> Result<usize> {
    let pw = sevenz_password(password);
    sevenz_rust2::decompress_file_with_password(path, dest, pw).map_err(map_7z)?;
    count_files(dest)
}

fn extract_rar(path: &Path, dest: &Path, password: Option<&str>) -> Result<usize> {
    let archive = match password {
        Some(pw) => unrar::Archive::with_password(path, pw),
        None => unrar::Archive::new(path),
    };
    let mut open = archive
        .open_for_processing()
        .map_err(|e| map_rar(&e.to_string()))?;
    let mut files = 0;
    while let Some(header) = open
        .read_header()
        .map_err(|e| map_rar(&e.to_string()))?
    {
        let is_file = header.entry().is_file();
        let encrypted = header.entry().is_encrypted();
        if encrypted && password.is_none() {
            return Err(ArchiveError::PasswordRequired);
        }
        open = if is_file {
            files += 1;
            header
                .extract_with_base(dest)
                .map_err(|e| map_rar(&e.to_string()))?
        } else {
            header.skip().map_err(|e| map_rar(&e.to_string()))?
        };
    }
    Ok(files)
}

/// Creates a zip at `dest` with the given files and folders (each added under its own name, folders with their
/// contents). With a password, every entry is AES-256 encrypted. Returns the number of files written.
pub fn create_zip(sources: &[PathBuf], dest: &Path, password: Option<&str>) -> Result<usize> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut zip = ZipWriter::new(File::create(dest)?);
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut files = 0;
    for source in sources {
        let parent = source.parent().unwrap_or(Path::new(""));
        match password {
            Some(pw) => {
                let opts = base.with_aes_encryption(AesMode::Aes256, pw);
                add_path(&mut zip, parent, source, opts, &mut files)?;
            }
            None => add_path(&mut zip, parent, source, base, &mut files)?,
        }
    }
    zip.finish().map_err(|e| ArchiveError::Failed(e.to_string()))?;
    Ok(files)
}

fn add_path(
    zip: &mut ZipWriter<File>,
    base: &Path,
    path: &Path,
    opts: FileOptions<'_, '_, ()>,
    files: &mut usize,
) -> Result<()> {
    let name = path
        .strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if path.is_dir() {
        zip.add_directory(format!("{name}/"), opts)
            .map_err(|e| ArchiveError::Failed(e.to_string()))?;
        for child in fs::read_dir(path)? {
            add_path(zip, base, &child?.path(), opts, files)?;
        }
    } else {
        zip.start_file(name, opts)
            .map_err(|e| ArchiveError::Failed(e.to_string()))?;
        let mut src = File::open(path)?;
        io::copy(&mut src, zip)?;
        *files += 1;
    }
    Ok(())
}

fn count_files(dir: &Path) -> Result<usize> {
    let mut n = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        n += if path.is_dir() { count_files(&path)? } else { 1 };
    }
    Ok(n)
}

fn sevenz_password(password: Option<&str>) -> sevenz_rust2::Password {
    match password {
        Some(pw) => sevenz_rust2::Password::new(pw),
        None => sevenz_rust2::Password::empty(),
    }
}

fn map_zip(e: zip::result::ZipError) -> ArchiveError {
    match e {
        zip::result::ZipError::InvalidPassword => ArchiveError::WrongPassword,
        other => ArchiveError::Failed(other.to_string()),
    }
}

fn map_zip_io(e: io::Error) -> ArchiveError {
    // A wrong AES password surfaces as an invalid-data read error when the entry is decrypted.
    if e.kind() == io::ErrorKind::InvalidData {
        ArchiveError::WrongPassword
    } else {
        ArchiveError::Failed(e.to_string())
    }
}

fn map_7z(e: sevenz_rust2::Error) -> ArchiveError {
    match e {
        sevenz_rust2::Error::PasswordRequired => ArchiveError::PasswordRequired,
        sevenz_rust2::Error::MaybeBadPassword(_) => ArchiveError::WrongPassword,
        other => ArchiveError::Failed(other.to_string()),
    }
}

fn map_rar(text: &str) -> ArchiveError {
    let lower = text.to_ascii_lowercase();
    if lower.contains("password") {
        ArchiveError::WrongPassword
    } else {
        ArchiveError::Failed(text.to_string())
    }
}
