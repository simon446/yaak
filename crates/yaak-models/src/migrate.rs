use crate::error::Error::MigrationError;
use crate::error::Result;
use include_dir::{Dir, DirEntry, include_dir};
use log::{debug, info};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha384};

static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

pub fn migrate_db(pool: &Pool<SqliteConnectionManager>) -> Result<()> {
    info!("Running database migrations");

    // Ensure the table exists
    // NOTE: Yaak used to use sqlx for migrations, so we need to mirror that table structure. We
    //  are writing checksum but not verifying because we want to be able to change migrations after
    //  a release in case something breaks.
    pool.get()?.execute(
        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version        BIGINT PRIMARY KEY,
            description    TEXT NOT NULL,
            installed_on   TIMESTAMP default CURRENT_TIMESTAMP NOT NULL,
            success        BOOLEAN                             NOT NULL,
            checksum       BLOB                                NOT NULL,
            execution_time BIGINT                              NOT NULL
        )",
        [],
    )?;

    // Read and sort all .sql files
    let mut entries = MIGRATIONS_DIR
        .entries()
        .into_iter()
        .filter(|e| e.path().extension().map(|ext| ext == "sql").unwrap_or(false))
        .collect::<Vec<_>>();

    // Ensure they're in the correct order
    entries.sort_by_key(|e| e.path());

    // Run each migration in a transaction
    let mut num_migrations = 0;
    let mut ran_migrations = 0;
    for entry in entries {
        num_migrations += 1;
        let mut conn = pool.get()?;
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match run_migration(entry, &mut tx) {
            Ok(ran) => {
                if ran {
                    ran_migrations += 1;
                }
                tx.commit()?
            }
            Err(e) => {
                let msg = format!(
                    "{} failed with {}",
                    entry.path().file_name().unwrap().to_str().unwrap(),
                    e.to_string()
                );
                tx.rollback()?;
                return Err(MigrationError(msg));
            }
        };
    }

    if ran_migrations == 0 {
        info!("No migrations to run out of {}", num_migrations);
    } else {
        info!("Ran {}/{} migrations", ran_migrations, num_migrations);
    }

    run_code_migrations(pool)?;

    Ok(())
}

/// Rust-coded migrations that run after the SQL migrations. Each is gated by a row in
/// `_sqlx_migrations` so it only runs once. Versions must not collide with SQL migration
/// versions.
fn run_code_migrations(pool: &Pool<SqliteConnectionManager>) -> Result<()> {
    run_code_migration(
        pool,
        "20260520000000",
        "path-placeholders-to-brace-syntax",
        migrate_path_placeholders_to_braces,
    )?;
    Ok(())
}

fn run_code_migration(
    pool: &Pool<SqliteConnectionManager>,
    version: &str,
    description: &str,
    body: fn(&mut rusqlite::Transaction) -> Result<()>,
) -> Result<()> {
    let mut conn = pool.get()?;
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let already_applied: Option<i64> = tx
        .query_row("SELECT 1 FROM _sqlx_migrations WHERE version = ?", [version], |r| r.get(0))
        .optional()?;
    if already_applied.is_some() {
        return Ok(());
    }

    let start = std::time::Instant::now();
    info!("Applying code migration {description}");
    body(&mut tx)?;
    let execution_time = start.elapsed().as_nanos() as i64;

    // Code migrations don't have a meaningful checksum.
    tx.execute(
        "INSERT INTO _sqlx_migrations (version, description, execution_time, checksum, success) VALUES (?, ?, ?, ?, ?)",
        params![version, description, execution_time, "0xCODE", true],
    )?;
    tx.commit()?;
    Ok(())
}

/// Rewrite the legacy `:name` path-placeholder convention to `{name}` for every saved
/// HTTP and WebSocket request. Touches both the `url` column and the JSON-encoded
/// `url_parameters` column. Idempotent: parameters without a `:` prefix are left alone.
fn migrate_path_placeholders_to_braces(tx: &mut rusqlite::Transaction) -> Result<()> {
    for table in ["http_requests", "websocket_requests"].iter() {
        let select_q = format!("SELECT id, url, url_parameters FROM {}", table);
        let rows: Vec<(String, String, String)> = {
            let mut stmt = tx.prepare(&select_q)?;
            let mapped = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let update_q = format!("UPDATE {} SET url = ?, url_parameters = ? WHERE id = ?", table);
        for (id, url, params_json) in rows {
            let Some((new_url, new_params_json)) =
                rewrite_one_request(&url, &params_json)
            else {
                continue;
            };
            tx.execute(update_q.as_str(), params![new_url, new_params_json, id])?;
        }
    }
    Ok(())
}

fn rewrite_one_request(url: &str, params_json: &str) -> Option<(String, String)> {
    let mut params: Vec<serde_json::Value> = serde_json::from_str(params_json).ok()?;

    // Gather rename pairs first. Sort longest-old-name first so a parameter `:foo` does
    // not eat a prefix of `:foobar` during URL rewriting.
    let mut renames: Vec<(String, String)> = Vec::new();
    for p in &params {
        let Some(name) = p.get("name").and_then(|v| v.as_str()) else { continue };
        if let Some(bare) = name.strip_prefix(':') {
            if !bare.is_empty() {
                renames.push((name.to_string(), format!("{{{}}}", bare)));
            }
        }
    }
    if renames.is_empty() {
        return None;
    }
    renames.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut new_url = url.to_string();
    for (old, new) in &renames {
        new_url = rewrite_url_for_param(&new_url, old, new);
    }

    for p in params.iter_mut() {
        let Some(obj) = p.as_object_mut() else { continue };
        let Some(name) = obj.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        if let Some((_, new)) = renames.iter().find(|(o, _)| o == &name) {
            obj.insert("name".into(), serde_json::Value::String(new.clone()));
        }
    }

    let new_params_json = serde_json::to_string(&params).ok()?;
    Some((new_url, new_params_json))
}

/// Replace every occurrence of `old` in `url` with `new`, but only where the match is
/// followed by a path-segment boundary (`/`, `?`, `#`, or end-of-string). This keeps a
/// rename of `:foo` from corrupting a literal `:foobar` substring.
fn rewrite_url_for_param(url: &str, old: &str, new: &str) -> String {
    let mut result = String::with_capacity(url.len() + new.len());
    let mut i = 0;
    while i < url.len() {
        if url[i..].starts_with(old) {
            let after = i + old.len();
            let boundary = after == url.len()
                || matches!(url.as_bytes()[after], b'/' | b'?' | b'#');
            if boundary {
                result.push_str(new);
                i = after;
                continue;
            }
        }
        let ch = url[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

#[cfg(test)]
mod placeholder_migration_tests {
    use super::{rewrite_one_request, rewrite_url_for_param};

    #[test]
    fn rewrite_url_anchors_at_boundaries() {
        assert_eq!(
            rewrite_url_for_param("https://example.com/users/:id/edit", ":id", "{id}"),
            "https://example.com/users/{id}/edit"
        );
    }

    #[test]
    fn rewrite_url_at_end_of_string() {
        assert_eq!(
            rewrite_url_for_param("https://example.com/users/:id", ":id", "{id}"),
            "https://example.com/users/{id}"
        );
    }

    #[test]
    fn rewrite_url_does_not_corrupt_prefixes() {
        // `:foo` should not match inside `:foobar`.
        assert_eq!(
            rewrite_url_for_param("https://example.com/x/:foobar", ":foo", "{foo}"),
            "https://example.com/x/:foobar"
        );
    }

    #[test]
    fn rewrite_url_before_query() {
        assert_eq!(
            rewrite_url_for_param("https://example.com/users/:id?q=x", ":id", "{id}"),
            "https://example.com/users/{id}?q=x"
        );
    }

    #[test]
    fn rewrite_request_full() {
        let url = "https://example.com/users/:id/posts/:postId?q=x";
        let params = r#"[
            {"name":":id","value":"42","enabled":true},
            {"name":":postId","value":"7","enabled":true},
            {"name":"q","value":"x","enabled":true}
        ]"#;
        let (new_url, new_params) = rewrite_one_request(url, params).expect("should change");
        assert_eq!(new_url, "https://example.com/users/{id}/posts/{postId}?q=x");
        let parsed: serde_json::Value = serde_json::from_str(&new_params).unwrap();
        assert_eq!(parsed[0]["name"], "{id}");
        assert_eq!(parsed[1]["name"], "{postId}");
        assert_eq!(parsed[2]["name"], "q");
    }

    #[test]
    fn rewrite_request_no_path_params_returns_none() {
        let url = "https://example.com/x";
        let params = r#"[{"name":"q","value":"1","enabled":true}]"#;
        assert!(rewrite_one_request(url, params).is_none());
    }

    #[test]
    fn rewrite_handles_prefix_overlap_with_longest_first() {
        let url = "https://example.com/:foo/:foobar";
        let params = r#"[
            {"name":":foo","value":"a"},
            {"name":":foobar","value":"b"}
        ]"#;
        let (new_url, _) = rewrite_one_request(url, params).expect("should change");
        assert_eq!(new_url, "https://example.com/{foo}/{foobar}");
    }
}

fn run_migration(migration_path: &DirEntry, tx: &mut rusqlite::Transaction) -> Result<bool> {
    let start = std::time::Instant::now();
    let (version, description) = split_migration_filename(migration_path.path().to_str().unwrap())
        .expect("Failed to parse migration filename");

    // Skip if already applied
    let row: Option<i64> = tx
        .query_row("SELECT 1 FROM _sqlx_migrations WHERE version = ?", [version.clone()], |r| {
            r.get(0)
        })
        .optional()?;

    if row.is_some() {
        debug!("Skipping already run migration {description}");
        return Ok(false); // Migration was already run
    }

    let sql =
        migration_path.as_file().unwrap().contents_utf8().expect("Failed to read migration file");
    info!("Applying migration {description}");

    // Split on `;`? → optional depending on how your SQL is structured
    tx.execute_batch(&sql)?;

    let execution_time = start.elapsed().as_nanos() as i64;
    let checksum = sha384_hex_prefixed(sql.as_bytes());

    // NOTE: The success column is never used. It's just there for sqlx compatibility.
    tx.execute(
        "INSERT INTO _sqlx_migrations (version, description, execution_time, checksum, success) VALUES (?, ?, ?, ?, ?)",
        params![version, description, execution_time, checksum, true],
    )?;

    Ok(true)
}

fn split_migration_filename(filename: &str) -> Option<(String, String)> {
    // Remove the .sql extension
    let trimmed = filename.strip_suffix(".sql")?;

    // Split on the first underscore
    let mut parts = trimmed.splitn(2, '_');
    let version = parts.next()?.to_string();
    let description = parts.next()?.to_string();

    Some((version, description))
}

fn sha384_hex_prefixed(input: &[u8]) -> String {
    let mut hasher = Sha384::new();
    hasher.update(input);
    let result = hasher.finalize();

    // Format as 0x... with uppercase hex
    format!("0x{}", hex::encode_upper(result))
}
