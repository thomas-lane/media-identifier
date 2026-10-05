//! Reference text from local files: subtitle or lyrics files in a folder.
//!
//! Used by tests, by the integrator's check against real episodes, and by anyone who has
//! subtitle files for a show already. Files are matched to episodes by an episode marker in the
//! file name (`S01E02`, `1x02`, `Season 1 Episode 2`, in the numbering of the episodes asked
//! for), else by the episode title appearing in the file name as whole words. Nothing is cached,
//! because the files may change and reading them is cheap.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use mi_types::{Episode, ProviderId, ReferenceText, TextKind};

use crate::names;
use crate::provider::{ReferenceProvider, ReferenceRequest, reference_text};

/// File extensions read as subtitles.
pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa"];
/// File extensions read as lyrics.
pub const LYRICS_EXTENSIONS: &[&str] = &["lrc", "txt"];

/// Subtitle and lyrics files from one folder.
#[derive(Debug, Clone)]
pub struct LocalReferences {
    files: Vec<PathBuf>,
}

impl LocalReferences {
    /// Lists the subtitle and lyrics files in `folder` and its immediate subfolders (for
    /// example `Season 01/`), sorted by path.
    pub fn from_folder(folder: &Path) -> crate::Result<Self> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(folder)? {
            let path = entry?.path();
            if path.is_dir() {
                for inner in std::fs::read_dir(&path)? {
                    let inner = inner?.path();
                    if inner.is_file() && kind_of(&inner).is_some() {
                        files.push(inner);
                    }
                }
            } else if kind_of(&path).is_some() {
                files.push(path);
            }
        }
        files.sort();
        Ok(Self { files })
    }

    /// Uses exactly these files.
    pub fn from_files(files: Vec<PathBuf>) -> Self {
        Self { files }
    }

    /// The files this provider reads.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The file for `episode`, if any: the first file whose marker equals the episode key, else
    /// the file whose name contains the episode title, preferring the longest title among all
    /// episodes that file's name contains (so `My Hero, Zero.srt` is not taken for `Zero`).
    pub fn file_for(&self, episode: &Episode, all: &[Episode]) -> Option<&Path> {
        let by_marker = self.files.iter().find(|f| {
            names::episode_marker(&stem(f)) == Some((episode.key.season, episode.key.number))
        });
        if let Some(f) = by_marker {
            return Some(f);
        }
        self.files
            .iter()
            .filter(|f| names::episode_marker(&stem(f)).is_none())
            .find(|f| {
                let name = stem(f);
                let longest = all
                    .iter()
                    .filter(|e| names::contains_words(&name, &e.title))
                    .max_by_key(|e| names::normalize(&e.title).len());
                longest.is_some_and(|e| e.provider_episode_id == episode.provider_episode_id)
            })
            .map(PathBuf::as_path)
    }

    fn read(
        &self,
        path: &Path,
        episode: &Episode,
        language: &str,
    ) -> crate::Result<Option<ReferenceText>> {
        let Some(kind) = kind_of(path) else {
            return Ok(None);
        };
        let content = crate::text::decode_bytes(&std::fs::read(path)?);
        let text = match kind {
            TextKind::Lyrics => {
                if content.contains("-->") {
                    crate::text::subtitle_to_dialogue(&content)
                } else if content.lines().any(|l| l.trim_start().starts_with('[')) {
                    crate::text::lrc_to_lyrics(&content)
                } else {
                    crate::text::plain_lyrics(&content)
                }
            }
            _ => crate::text::subtitle_to_dialogue(&content),
        };
        if text.is_empty() {
            return Ok(None);
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Some(reference_text(
            episode,
            kind,
            ProviderId::Local,
            name,
            text,
            language,
        )))
    }
}

#[async_trait]
impl ReferenceProvider for LocalReferences {
    fn id(&self) -> ProviderId {
        ProviderId::Local
    }

    async fn reference_texts(
        &self,
        request: &ReferenceRequest<'_>,
    ) -> crate::Result<Vec<ReferenceText>> {
        let mut out = Vec::new();
        for episode in request.episodes {
            request.check_cancel()?;
            if let Some(path) = self.file_for(episode, request.episodes)
                && let Some(text) = self.read(path, episode, request.language)?
            {
                out.push(text);
            }
        }
        Ok(out)
    }
}

fn kind_of(path: &Path) -> Option<TextKind> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if SUBTITLE_EXTENSIONS.contains(&ext.as_str()) {
        Some(TextKind::Subtitles)
    } else if LYRICS_EXTENSIONS.contains(&ext.as_str()) {
        Some(TextKind::Lyrics)
    } else {
        None
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
