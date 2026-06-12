use std::{
    cmp::Reverse,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use tokei::{Config, Languages, Sort};

use crate::{
    model::{RealizationStatus, SourceLanguageLoc, SourceLoc, SourceScanStats},
    nix::NixRunner,
};

const TOKEI_POLICY_HASH: &str = "tokei-v14-source-graph-v2";

#[derive(Clone)]
pub(super) struct SourceMeasurement {
    pub(super) realization_status: RealizationStatus,
    pub(super) loc: Option<SourceLoc>,
}

pub(super) struct LocCache {
    conn: Connection,
}

pub(super) struct LocMeasureContext<'a> {
    pub(super) runner: &'a mut NixRunner,
    pub(super) loc_cache: &'a mut HashMap<String, SourceMeasurement>,
    pub(super) persistent_loc_cache: Option<&'a mut LocCache>,
    pub(super) loc_stats: &'a mut SourceScanStats,
}

fn count_source_path(path: &Path) -> Result<Option<SourceLoc>> {
    if !path.exists() {
        return Ok(None);
    }

    let archive_dir;
    let count_path = if path.is_file() && is_archive(path) {
        archive_dir = unpack_archive(path)?;
        archive_dir.path().to_path_buf()
    } else {
        path.to_path_buf()
    };

    let included = [count_path.to_string_lossy().to_string()];
    let included_refs: Vec<_> = included.iter().map(String::as_str).collect();
    let excluded = ["/.git/", "/target/", "/result", "/node_modules/"];
    let config = Config {
        hidden: Some(true),
        no_ignore: Some(true),
        treat_doc_strings_as_comments: Some(true),
        ..Config::default()
    };

    let mut languages = Languages::new();
    languages.get_statistics(&included_refs, &excluded, &config);
    let total = languages.total();

    let mut language_rows = Vec::new();
    for (language, mut reports) in languages {
        reports.sort_by(Sort::Lines);
        language_rows.push(SourceLanguageLoc {
            language: language.to_string(),
            files: reports.reports.len() as i64,
            loc_total: (reports.code + reports.comments + reports.blanks) as i64,
            loc_code: reports.code as i64,
            loc_comments: reports.comments as i64,
            loc_blank: reports.blanks as i64,
        });
    }
    language_rows.sort_by_key(|row| Reverse(row.loc_total));

    Ok(Some(SourceLoc {
        policy_hash: TOKEI_POLICY_HASH.to_string(),
        counter: "tokei".to_string(),
        loc_total: (total.code + total.comments + total.blanks) as i64,
        loc_code: total.code as i64,
        loc_comments: total.comments as i64,
        loc_blank: total.blanks as i64,
        languages: language_rows,
    }))
}

impl LocCache {
    pub(super) fn open() -> Result<Self> {
        let cache_dir = cache_dir().context("failed to determine LOC cache directory")?;
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("failed to create LOC cache directory {}", cache_dir.display()))?;
        let conn = Connection::open(cache_dir.join("loc-cache.sqlite"))?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS loc_cache (
                source_path TEXT NOT NULL,
                policy_hash TEXT NOT NULL,
                counter TEXT NOT NULL,
                realization_status TEXT NOT NULL,
                loc_total INTEGER,
                loc_code INTEGER,
                loc_comments INTEGER,
                loc_blank INTEGER,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (source_path, policy_hash, counter)
            );
            CREATE TABLE IF NOT EXISTS loc_cache_language (
                source_path TEXT NOT NULL,
                policy_hash TEXT NOT NULL,
                counter TEXT NOT NULL,
                language TEXT NOT NULL,
                files INTEGER NOT NULL,
                loc_total INTEGER NOT NULL,
                loc_code INTEGER NOT NULL,
                loc_comments INTEGER NOT NULL,
                loc_blank INTEGER NOT NULL,
                PRIMARY KEY (source_path, policy_hash, counter, language),
                FOREIGN KEY (source_path, policy_hash, counter)
                    REFERENCES loc_cache(source_path, policy_hash, counter)
                    ON DELETE CASCADE
            );
            ",
        )?;
        Ok(Self { conn })
    }

    fn get(&self, source_path: &str) -> Result<Option<SourceMeasurement>> {
        let row = self
            .conn
            .query_row(
                "SELECT realization_status, loc_total, loc_code, loc_comments, loc_blank FROM loc_cache WHERE source_path = ?1 AND policy_hash = ?2 AND counter = ?3",
                params![source_path, TOKEI_POLICY_HASH, "tokei"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?;

        let Some((realization_status, loc_total, loc_code, loc_comments, loc_blank)) = row else {
            return Ok(None);
        };
        let loc = if let (Some(loc_total), Some(loc_code), Some(loc_comments), Some(loc_blank)) =
            (loc_total, loc_code, loc_comments, loc_blank)
        {
            let mut stmt = self.conn.prepare(
                "SELECT language, files, loc_total, loc_code, loc_comments, loc_blank FROM loc_cache_language WHERE source_path = ?1 AND policy_hash = ?2 AND counter = ?3 ORDER BY loc_total DESC",
            )?;
            let languages = stmt
                .query_map(params![source_path, TOKEI_POLICY_HASH, "tokei"], |row| {
                    Ok(SourceLanguageLoc {
                        language: row.get(0)?,
                        files: row.get(1)?,
                        loc_total: row.get(2)?,
                        loc_code: row.get(3)?,
                        loc_comments: row.get(4)?,
                        loc_blank: row.get(5)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Some(SourceLoc {
                policy_hash: TOKEI_POLICY_HASH.to_string(),
                counter: "tokei".to_string(),
                loc_total,
                loc_code,
                loc_comments,
                loc_blank,
                languages,
            })
        } else {
            None
        };
        Ok(Some(SourceMeasurement {
            realization_status: realization_status.into(),
            loc,
        }))
    }

    fn put(&mut self, source_path: &str, measurement: &SourceMeasurement) -> Result<()> {
        let loc = measurement.loc.as_ref();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO loc_cache (source_path, policy_hash, counter, realization_status, loc_total, loc_code, loc_comments, loc_blank, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
            params![
                source_path,
                TOKEI_POLICY_HASH,
                "tokei",
                measurement.realization_status.as_str(),
                loc.map(|loc| loc.loc_total),
                loc.map(|loc| loc.loc_code),
                loc.map(|loc| loc.loc_comments),
                loc.map(|loc| loc.loc_blank),
            ],
        )?;
        tx.execute(
            "DELETE FROM loc_cache_language WHERE source_path = ?1 AND policy_hash = ?2 AND counter = ?3",
            params![source_path, TOKEI_POLICY_HASH, "tokei"],
        )?;
        if let Some(loc) = loc {
            for language in &loc.languages {
                tx.execute(
                    "INSERT INTO loc_cache_language (source_path, policy_hash, counter, language, files, loc_total, loc_code, loc_comments, loc_blank) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        source_path,
                        TOKEI_POLICY_HASH,
                        "tokei",
                        language.language,
                        language.files,
                        language.loc_total,
                        language.loc_code,
                        language.loc_comments,
                        language.loc_blank,
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn cache_dir() -> Option<PathBuf> {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(cache_home).join("klocc"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/klocc"))
}

pub(super) fn measure_source_path(
    path: &Path,
    runner: &mut NixRunner,
    cache: &mut HashMap<String, SourceMeasurement>,
    mut persistent_cache: Option<&mut LocCache>,
    stats: &mut SourceScanStats,
) -> Result<SourceMeasurement> {
    let key = path.to_string_lossy().to_string();
    if let Some(measurement) = cache.get(&key) {
        stats.loc_memory_cache_hits += 1;
        return Ok(measurement.clone());
    }
    if key.starts_with("/nix/store/")
        && let Some(cache_db) = &mut persistent_cache
        && let Some(measurement) = cache_db.get(&key)?
    {
        stats.loc_persistent_cache_hits += 1;
        cache.insert(key, measurement.clone());
        return Ok(measurement);
    }

    stats.loc_cache_misses += 1;
    let started = Instant::now();
    if crate::time::timings_enabled() {
        eprintln!("klocc: progress source.measure start path={key}");
    }
    let realization_status = realize_source_path(path, runner);
    if crate::time::timings_enabled() {
        eprintln!(
            "klocc: progress source.measure realized status={} path={} elapsed={:.3}ms",
            realization_status.as_str(),
            key,
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }
    let loc = count_source_path(path)?;
    let measurement = SourceMeasurement {
        realization_status,
        loc,
    };
    if key.starts_with("/nix/store/")
        && let Some(cache_db) = &mut persistent_cache
    {
        cache_db.put(&key, &measurement)?;
        stats.loc_cache_stores += 1;
    }
    cache.insert(key, measurement.clone());
    if crate::time::timings_enabled() {
        eprintln!(
            "klocc: progress source.measure done path={} elapsed={:.3}ms",
            path.display(),
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }
    Ok(measurement)
}

fn realize_source_path(path: &Path, runner: &mut NixRunner) -> RealizationStatus {
    if path.exists() {
        return "available".into();
    }
    let path_text = path.to_string_lossy();
    if !path_text.starts_with("/nix/store/") {
        return "missing".into();
    }
    match runner.realize_store_path(&path_text) {
        Ok(()) if path.exists() => "realized".into(),
        Ok(()) => "realize-command-succeeded-but-missing".into(),
        Err(_) => "missing".into(),
    }
}

fn is_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar")
        || name.ends_with(".tar.xz")
        || name.ends_with(".tar.bz2")
}

fn unpack_archive(path: &Path) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir().context("failed to create temporary source archive extraction directory")?;
    let output = Command::new("tar")
        .arg("-xf")
        .arg(path)
        .arg("-C")
        .arg(dir.path())
        .output()
        .with_context(|| format!("failed to spawn tar for {}", path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "failed to unpack source archive {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(dir)
}
