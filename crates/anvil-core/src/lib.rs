use chrono::{DateTime, Utc};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

const FORMAT_VERSION: u32 = 1;
const META_DIR: &str = ".anvil";

#[derive(Debug, Error)]
pub enum AnvilError {
    #[error("not an Anvil repository: {0}")]
    NotRepository(PathBuf),
    #[error("Anvil repository already exists: {0}")]
    AlreadyRepository(PathBuf),
    #[error("commit not found: {0}")]
    CommitNotFound(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AnvilError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub format_version: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub id: String,
    pub parent: Option<String>,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub object: String,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct Status {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: usize,
}

impl Status {
    pub fn is_clean(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        let root = absolute(path.as_ref())?;
        let meta = root.join(META_DIR);
        if meta.exists() {
            return Err(AnvilError::AlreadyRepository(root));
        }

        fs::create_dir_all(meta.join("objects"))?;
        fs::create_dir_all(meta.join("commits"))?;
        fs::create_dir_all(meta.join("refs"))?;

        let config = RepoConfig {
            format_version: FORMAT_VERSION,
            created_at: Utc::now(),
        };
        write_json_atomic(&meta.join("repo.json"), &config)?;
        fs::write(meta.join("refs/HEAD"), b"")?;

        let ignore_path = root.join(".anvilignore");
        if !ignore_path.exists() {
            fs::write(
                ignore_path,
                "# Anvil ignore rules\n.anvil/\nlogs/\ncache/\ncrash-reports/\n**/session.lock\n",
            )?;
        }

        Ok(Self { root })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = absolute(path.as_ref())?;
        if !root.join(META_DIR).join("repo.json").is_file() {
            return Err(AnvilError::NotRepository(root));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn head_id(&self) -> Result<Option<String>> {
        let value = fs::read_to_string(self.meta().join("refs/HEAD"))?;
        let value = value.trim();
        Ok((!value.is_empty()).then(|| value.to_owned()))
    }

    pub fn head(&self) -> Result<Option<Commit>> {
        self.head_id()?
            .map(|id| self.read_commit(&id))
            .transpose()
    }

    pub fn status(&self) -> Result<Status> {
        let current = self.scan()?;
        let previous = self.head()?.map(|c| c.files).unwrap_or_default();
        let mut status = Status::default();

        for (path, entry) in &current {
            match previous.get(path) {
                None => status.added.push(path.clone()),
                Some(old) if old.object != entry.object => status.modified.push(path.clone()),
                Some(_) => status.unchanged += 1,
            }
        }

        for path in previous.keys() {
            if !current.contains_key(path) {
                status.deleted.push(path.clone());
            }
        }

        Ok(status)
    }

    pub fn commit(&self, message: impl Into<String>) -> Result<Commit> {
        let message = message.into();
        let parent = self.head_id()?;
        let files = self.scan_and_store()?;
        let created_at = Utc::now();

        let material = CommitMaterial {
            parent: parent.as_deref(),
            message: &message,
            created_at: created_at.clone(),
            files: &files,
        };
        let encoded = serde_json::to_vec(&material)?;
        let id = hash_bytes(&encoded);
        let commit = Commit {
            id: id.clone(),
            parent,
            message,
            created_at,
            files,
        };

        write_json_atomic(&self.meta().join("commits").join(format!("{id}.json")), &commit)?;
        write_atomic(&self.meta().join("refs/HEAD"), id.as_bytes())?;
        Ok(commit)
    }

    pub fn log(&self) -> Result<Vec<Commit>> {
        let mut commits = Vec::new();
        let mut cursor = self.head_id()?;
        let mut seen = BTreeSet::new();

        while let Some(id) = cursor {
            if !seen.insert(id.clone()) {
                break;
            }
            let commit = self.read_commit(&id)?;
            cursor = commit.parent.clone();
            commits.push(commit);
        }
        Ok(commits)
    }

    pub fn read_commit(&self, id: &str) -> Result<Commit> {
        let path = self.meta().join("commits").join(format!("{id}.json"));
        if !path.is_file() {
            return Err(AnvilError::CommitNotFound(id.to_owned()));
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn meta(&self) -> PathBuf {
        self.root.join(META_DIR)
    }

    fn scan(&self) -> Result<BTreeMap<String, FileEntry>> {
        let mut files = BTreeMap::new();
        for path in self.walk_files() {
            let rel = relative_string(&self.root, &path);
            let (hash, size) = hash_file(&path)?;
            files.insert(rel, FileEntry { object: hash, size });
        }
        Ok(files)
    }

    fn scan_and_store(&self) -> Result<BTreeMap<String, FileEntry>> {
        let mut files = BTreeMap::new();
        for path in self.walk_files() {
            let rel = relative_string(&self.root, &path);
            let bytes = fs::read(&path)?;
            let hash = hash_bytes(&bytes);
            let object_path = self.meta().join("objects").join(&hash[..2]).join(&hash[2..]);
            if !object_path.exists() {
                if let Some(parent) = object_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                write_atomic(&object_path, &bytes)?;
            }
            files.insert(
                rel,
                FileEntry {
                    object: hash,
                    size: bytes.len() as u64,
                },
            );
        }
        Ok(files)
    }

    fn walk_files(&self) -> Vec<PathBuf> {
        let mut builder = WalkBuilder::new(&self.root);
        builder.hidden(false).git_ignore(false).git_exclude(false).parents(false);

        let ignore_file = self.root.join(".anvilignore");
        if ignore_file.is_file() {
            builder.add_custom_ignore_filename(".anvilignore");
        }

        let meta = self.meta();
        builder
            .filter_entry(move |entry| entry.path() != meta)
            .build()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
            .map(|entry| entry.into_path())
            .collect()
    }
}

#[derive(Serialize)]
struct CommitMaterial<'a> {
    parent: Option<&'a str>,
    message: &'a str,
    created_at: DateTime<Utc>,
    files: &'a BTreeMap<String, FileEntry>,
}

fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        path.canonicalize()
    } else {
        fs::create_dir_all(path)?;
        path.canonicalize()
    }
}

fn hash_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        size += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn relative_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &data)?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)
}
