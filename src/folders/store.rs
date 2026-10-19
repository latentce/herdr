//! Saves folder state next to the session file, in its own file, so stock
//! Herdr can rewrite the session without dropping folders.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::model::{parse_folder_id, public_folder_id, Folder, FolderState, Item};

pub(crate) const FOLDER_FILE_NAME: &str = "folders.json";
/// The folder file as loaded, kept when it names workspaces that did not come
/// back, so it can be restored together with a recovered session.
pub(crate) const FOLDER_RECOVERY_FILE_NAME: &str = "folders-recovery.json";
const FOLDER_FILE_VERSION: u32 = 1;

#[derive(Debug, Default)]
pub(crate) struct FolderStorage {
    /// Saved data has been read (or there was none to read).
    pub(crate) loaded: bool,
    /// Where folder data is saved; `None` disables persistence.
    pub(crate) path: Option<PathBuf>,
    /// A folder file exists, so clearing the last folder must be saved too.
    pub(crate) has_file: bool,
    /// Background writer; saves never block the server loop.
    writer: FolderWriter,
}

#[derive(Debug, Default)]
struct FolderWriter {
    /// The latest unwritten data, and whether a writer thread is running.
    queue: Arc<Mutex<(Option<FolderFile>, bool)>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FolderStorage {
    /// Writes `file` in the background. Data queued while a write runs
    /// replaces older unwritten data.
    pub(crate) fn queue(&mut self, file: FolderFile) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.has_file = true;
        let queue = Arc::clone(&self.writer.queue);
        let Ok(mut guard) = queue.lock() else {
            return write_file(&path, &file);
        };
        guard.0 = Some(file);
        if guard.1 {
            return;
        }
        guard.1 = true;
        drop(guard);
        if let Some(thread) = self.writer.thread.take() {
            let _ = thread.join();
        }
        let worker = Arc::clone(&queue);
        let worker_path = path.clone();
        match std::thread::Builder::new()
            .name("herdr-folder-save".into())
            .spawn(move || drain(&worker, &worker_path))
        {
            Ok(thread) => self.writer.thread = Some(thread),
            Err(err) => {
                tracing::warn!(err = %err, "failed to spawn folder save thread; saving inline");
                drain(&queue, &path);
            }
        }
    }

    /// Waits until queued data is on disk.
    pub(crate) fn flush(&mut self) {
        if let Some(thread) = self.writer.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Writes queued data until none is left, then marks the writer idle.
fn drain(queue: &Mutex<(Option<FolderFile>, bool)>, path: &Path) {
    loop {
        let file = match queue.lock() {
            Ok(mut guard) => match guard.0.take() {
                Some(file) => file,
                None => {
                    guard.1 = false;
                    return;
                }
            },
            Err(_) => return,
        };
        write_file(path, &file);
    }
}

fn write_file(path: &Path, file: &FolderFile) {
    if let Err(err) = crate::persist::save_json_to_path(path, file) {
        tracing::warn!(path = %path.display(), err = %err, "failed to save workspace folders");
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FolderFile {
    version: u32,
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    folders: Vec<FolderFileEntry>,
    /// Combined order of folders (`fN`) and loose workspaces (`wN`).
    #[serde(default)]
    order: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FolderFileEntry {
    id: String,
    name: String,
    #[serde(default)]
    workspaces: Vec<String>,
}

/// Folder data for the active named session.
pub(crate) fn session_folder_path() -> PathBuf {
    crate::session::data_dir().join(FOLDER_FILE_NAME)
}

/// Copies the folder file to its recovery copy, replacing an older one.
pub(crate) fn keep_recovery_copy(path: &Path) -> std::io::Result<PathBuf> {
    let recovery = path.with_file_name(FOLDER_RECOVERY_FILE_NAME);
    std::fs::copy(path, &recovery)?;
    Ok(recovery)
}

/// Reads saved folders into `state`. Returns `false` when there is no file.
pub(crate) fn load(state: &mut FolderState, path: &Path) -> std::io::Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let file: FolderFile = serde_json::from_str(&text).map_err(std::io::Error::other)?;
    let folders = file
        .folders
        .into_iter()
        .filter_map(|entry| {
            let id = parse_folder_id(&entry.id)?;
            let name = super::model::normalize_folder_name(&entry.name)?;
            Some(Folder {
                id,
                name,
                members: entry.workspaces,
            })
        })
        .collect();
    let order = file
        .order
        .into_iter()
        .map(|id| match parse_folder_id(&id) {
            Some(folder) => Item::Folder(folder),
            None => Item::Workspace(id),
        })
        .collect();
    state.restore(folders, order, file.next_id);
    Ok(true)
}

impl FolderFile {
    pub(crate) fn from_state(state: &FolderState) -> Self {
        Self {
            version: FOLDER_FILE_VERSION,
            next_id: state.next_id(),
            folders: state
                .folders_in_order()
                .map(|(_, folder)| FolderFileEntry {
                    id: public_folder_id(folder.id),
                    name: folder.name.clone(),
                    workspaces: folder.members.clone(),
                })
                .collect(),
            order: state
                .order()
                .iter()
                .map(|item| match item {
                    Item::Workspace(id) => id.clone(),
                    Item::Folder(id) => public_folder_id(*id),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folders::model::WorkspaceFact;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "herdr-folders-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn fact(id: &str) -> WorkspaceFact {
        WorkspaceFact {
            id: id.into(),
            worktree: None,
        }
    }

    #[test]
    fn saved_folders_round_trip_with_ids_order_and_counter() {
        let dir = temp_dir("round-trip");
        let path = dir.join(FOLDER_FILE_NAME);
        let mut state = FolderState::default();
        state.reconcile(vec![fact("w1"), fact("w2"), fact("w3")]);
        let (first, _) = state
            .create("backend".into(), &["w2".into()], None)
            .expect("create");
        let (second, _) = state.create("empty".into(), &[], None).expect("create");
        state.delete(second).expect("delete");
        let (third, _) = state.create("later".into(), &[], None).expect("create");
        let mut storage = FolderStorage {
            path: Some(path.clone()),
            ..FolderStorage::default()
        };
        storage.queue(FolderFile::from_state(&state));
        storage.flush();

        let mut restored = FolderState::default();
        assert!(load(&mut restored, &path).expect("load"));
        restored.reconcile(vec![fact("w1"), fact("w2"), fact("w3")]);
        assert_eq!(
            restored.folder(first).map(|f| f.name.as_str()),
            Some("backend")
        );
        assert_eq!(restored.folder_of("w2"), Some(first));
        assert_eq!(restored.order(), state.order());
        let (next, _) = restored.create("new".into(), &[], None).expect("create");
        assert!(next > third, "folder ids are never reused");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let dir = temp_dir("missing");
        let mut state = FolderState::default();
        assert!(!load(&mut state, &dir.join(FOLDER_FILE_NAME)).expect("load"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
