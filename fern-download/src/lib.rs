//! Download and verification primitives for Fern.
//!
//! The downloader owns network and filesystem behavior while the UI receives
//! only serialized [`DownloadEvent`] values through the core boundary.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::sync::Semaphore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadEvent {
    TaskStarted { total_files: u64, total_bytes: u64 },
    FileDone { path: String, bytes: u64 },
    Progress { done_bytes: u64, speed_bps: u64 },
    TaskFinished { failed: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    pub path: PathBuf,
    pub url: Url,
    pub sha1: String,
    pub size: u64,
}

impl DownloadTask {
    pub fn new(
        path: impl Into<PathBuf>,
        url: &str,
        sha1: impl Into<String>,
        size: u64,
    ) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            url: Url::parse(url).context("invalid download URL")?,
            sha1: sha1.into().to_ascii_lowercase(),
            size,
        })
    }
}

pub trait DownloadSource: Send + Sync {
    fn rewrite(&self, official: &Url) -> Url;
}

#[derive(Debug, Default)]
pub struct OfficialSource;

impl DownloadSource for OfficialSource {
    fn rewrite(&self, official: &Url) -> Url {
        official.clone()
    }
}

#[derive(Debug, Default)]
pub struct BmclapiSource;

impl DownloadSource for BmclapiSource {
    fn rewrite(&self, official: &Url) -> Url {
        let Some(host) = official.host_str() else {
            return official.clone();
        };
        let replacement = match host {
            "libraries.minecraft.net" => "bmclapi2.bangbang93.com",
            "piston-meta.mojang.com" => "bmclapi2.bangbang93.com",
            "piston-data.mojang.com" => "bmclapi2.bangbang93.com",
            "resources.download.minecraft.net" => "bmclapi2.bangbang93.com",
            _ => return official.clone(),
        };
        let mut rewritten = official.clone();
        let _ = rewritten.set_host(Some(replacement));
        if host == "libraries.minecraft.net" {
            rewritten.set_path(&format!("/maven{}", official.path()));
        } else if host == "resources.download.minecraft.net" {
            rewritten.set_path(&format!("/assets{}", official.path()));
        }
        rewritten
    }
}

pub fn sha1_matches(bytes: &[u8], expected: &str) -> bool {
    let digest = Sha1::digest(bytes);
    let actual = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    actual.eq_ignore_ascii_case(expected)
}

pub async fn verify_file(path: &Path, expected_sha1: &str, expected_size: u64) -> Result<bool> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(bytes.len() as u64 == expected_size && sha1_matches(&bytes, expected_sha1))
}

pub fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "artifact path escapes data root: {}",
            relative.display()
        ));
    }
    Ok(root.join(relative))
}

#[derive(Clone)]
pub struct DownloadClient {
    client: reqwest::Client,
    sources: Vec<Arc<dyn DownloadSource>>,
    semaphore: Arc<Semaphore>,
}

impl DownloadClient {
    pub fn new(sources: Vec<Arc<dyn DownloadSource>>, concurrency: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            sources: if sources.is_empty() {
                vec![Arc::new(OfficialSource)]
            } else {
                sources
            },
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
        }
    }

    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        let url = Url::parse(url).context("invalid download URL")?;
        let mut last_error = None;
        for source in &self.sources {
            let rewritten = source.rewrite(&url);
            match self.client.get(rewritten).send().await {
                Ok(response) if response.status().is_success() => match response.bytes().await {
                    Ok(bytes) => return Ok(bytes.to_vec()),
                    Err(error) => last_error = Some(error.into()),
                },
                Ok(response) => {
                    last_error = Some(anyhow!("download failed with HTTP {}", response.status()))
                }
                Err(error) => last_error = Some(error.into()),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("no download source configured")))
    }

    pub async fn download(
        &self,
        task: &DownloadTask,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        self.download_all(vec![task.clone()], events).await
    }

    pub async fn download_all(
        &self,
        tasks: Vec<DownloadTask>,
        events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        let total_bytes = tasks.iter().map(|task| task.size).sum();
        let _ = events.send(DownloadEvent::TaskStarted {
            total_files: tasks.len() as u64,
            total_bytes,
        });
        let started = Instant::now();
        let mut jobs = tokio::task::JoinSet::new();
        for task in tasks {
            let client = self.clone();
            jobs.spawn(async move {
                let result = client.download_one(&task).await;
                (task, result)
            });
        }

        let mut done_bytes = 0u64;
        let mut failed = Vec::new();
        while let Some(joined) = jobs.join_next().await {
            match joined {
                Ok((task, Ok(()))) => {
                    done_bytes = done_bytes.saturating_add(task.size);
                    let speed =
                        (done_bytes as f64 / started.elapsed().as_secs_f64().max(0.001)) as u64;
                    let _ = events.send(DownloadEvent::FileDone {
                        path: task.path.display().to_string(),
                        bytes: task.size,
                    });
                    let _ = events.send(DownloadEvent::Progress {
                        done_bytes,
                        speed_bps: speed,
                    });
                }
                Ok((task, Err(_))) => failed.push(task.path.display().to_string()),
                Err(error) => failed.push(format!("download worker: {error}")),
            }
        }
        let _ = events.send(DownloadEvent::TaskFinished {
            failed: failed.clone(),
        });
        if failed.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("{} files failed to download", failed.len()))
        }
    }

    async fn download_one(&self, task: &DownloadTask) -> Result<()> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .context("download semaphore closed")?;

        if verify_file(&task.path, &task.sha1, task.size).await? {
            return Ok(());
        }

        if let Some(parent) = task.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let temporary = task.path.with_extension("part");
        let mut last_error = None;
        for source in &self.sources {
            let url = source.rewrite(&task.url);
            match self.client.get(url).send().await {
                Ok(response) if response.status().is_success() => match response.bytes().await {
                    Ok(bytes)
                        if bytes.len() as u64 == task.size && sha1_matches(&bytes, &task.sha1) =>
                    {
                        tokio::fs::write(&temporary, &bytes).await?;
                        if tokio::fs::try_exists(&task.path).await? {
                            tokio::fs::remove_file(&task.path).await?;
                        }
                        tokio::fs::rename(&temporary, &task.path).await?;
                        return Ok(());
                    }
                    Ok(_) => {
                        last_error = Some(anyhow!("checksum or size mismatch for {}", task.url))
                    }
                    Err(error) => last_error = Some(error.into()),
                },
                Ok(response) => {
                    last_error = Some(anyhow!("download failed with HTTP {}", response.status()))
                }
                Err(error) => last_error = Some(error.into()),
            }
        }
        let _ = tokio::fs::remove_file(&temporary).await;
        Err(last_error.unwrap_or_else(|| anyhow!("no download source configured")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verifies_content_by_size_and_sha1() {
        let root = std::env::temp_dir().join(format!("fern-download-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create test directory");
        let path = root.join("client.jar");
        tokio::fs::write(&path, b"fern")
            .await
            .expect("write fixture");
        assert!(
            verify_file(&path, "654edb122a04602f918500d59b1d6fc37b9d0c01", 4)
                .await
                .expect("verify fixture")
        );
        assert!(
            !verify_file(&path, "0000000000000000000000000000000000000000", 4)
                .await
                .expect("verify mismatch")
        );
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove test directory");
    }

    #[test]
    fn rejects_paths_that_escape_the_data_root() {
        assert!(safe_join(Path::new("/fern"), Path::new("assets/client.jar")).is_ok());
        assert!(safe_join(Path::new("/fern"), Path::new("../outside")).is_err());
        assert!(safe_join(Path::new("/fern"), Path::new("/outside")).is_err());
    }

    #[test]
    fn rewrites_known_mirrors() {
        let source = BmclapiSource;
        let official = Url::parse("https://libraries.minecraft.net/com/mojang/test.jar").unwrap();
        let mirror = source.rewrite(&official);
        assert_eq!(mirror.host_str(), Some("bmclapi2.bangbang93.com"));
        assert_eq!(mirror.path(), "/maven/com/mojang/test.jar");
    }
}
