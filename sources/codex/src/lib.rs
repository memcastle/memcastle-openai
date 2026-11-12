//! Codex rollout history, read from the JSONL transcripts Codex writes under `~/.codex/sessions`.
//!
//! The reader keeps what the user and assistant said, plus compact tool-call markers.
//! It intentionally leaves out reasoning, tool output, command/file contents and injected context: those are not the
//! conversation and can carry secrets or a copy of material that belongs to another source.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

wit_bindgen::generate!({
    path: "wit",
    world: "reconciled-source",
});

use exports::memcastle::source::adapter::Guest;
use exports::memcastle::source::reconciliation::{
    self, Acquisition, Decision, DiscoveryPage, KnownDocument,
};
use memcastle::source::types::{
    Candidate, CanonicalDocument, Discovery, RawDocument, Segment, SourceError, SourceKind,
    SourceRef,
};

const NAME: &str = "codex";
const TITLE_CHARS: usize = 80;

struct Codex;

struct Entry {
    key: String,
    mtime_ns: i64,
}

fn default_root() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".codex").join("sessions"))
}

fn mtime_ns(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|time| i64::try_from(time.as_nanos()).ok())
        .unwrap_or(0)
}

/// Rollouts are three date directories deep; limiting the walk prevents an unexpected user tree from widening access.
fn collect(dir: &Path, prefix: &str, depth: u8, out: &mut Vec<Entry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if kind.is_dir() && depth < 3 {
            collect(&entry.path(), &key, depth + 1, out);
        } else if kind.is_file()
            && depth == 3
            && name.starts_with("rollout-")
            && name.ends_with(".jsonl")
            && let Ok(metadata) = entry.metadata()
        {
            out.push(Entry {
                key,
                mtime_ns: mtime_ns(&metadata),
            });
        }
    }
}

fn parse_cursor(cursor: &str) -> Result<Option<(i64, String)>, SourceError> {
    let invalid = |message: &str| SourceError::CursorInvalid(message.to_string());
    let value: Value =
        serde_json::from_str(cursor).map_err(|_| invalid("the cursor is not JSON"))?;
    if value.is_null() || value.get("empty") == Some(&Value::Bool(true)) {
        return Ok(None);
    }
    let mtime_ns = value
        .get("mtime_ns")
        .and_then(Value::as_i64)
        .ok_or_else(|| invalid("`mtime_ns` is missing or not an integer"))?;
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty() && !key.contains(".."))
        .ok_or_else(|| invalid("`key` is missing or not a rollout path"))?;
    Ok(Some((mtime_ns, key.to_string())))
}

fn path_of(source: &SourceRef, key: &str) -> PathBuf {
    key.split('/')
        .fold(PathBuf::from(&source.locator), |path, part| path.join(part))
}

fn revision_of(body: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in body.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}-{}", body.len())
}

fn payload(entry: &Value) -> &Value {
    entry.get("payload").unwrap_or(entry)
}

fn text_from(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                let kind = part.get("type").and_then(Value::as_str)?;
                // These are the only content blocks authored for the conversation; reasoning and tool output are excluded.
                matches!(kind, "input_text" | "output_text" | "text")
                    .then(|| {
                        part.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string()
                    })
                    .filter(|text| !text.is_empty())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(TITLE_CHARS)
        .collect()
}

fn absolute_path(value: &str) -> Option<PathBuf> {
    use std::path::Component;
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => (),
            other => result.push(other.as_os_str()),
        }
    }
    Some(result)
}

fn project(source: &SourceRef) -> Option<PathBuf> {
    source
        .options
        .iter()
        .find(|(key, _)| key == "project")
        .and_then(|(_, value)| absolute_path(value))
}

fn valid_key(key: &str) -> bool {
    let parts: Vec<_> = key.split('/').collect();
    parts.len() == 4
        && parts[..3]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && parts[3].starts_with("rollout-")
        && parts[3].ends_with(".jsonl")
        && !key.contains("..")
        && !key.contains('\\')
}

fn snapshot(source: &SourceRef, key: &str) -> Result<Option<RawDocument>, SourceError> {
    use std::io::Read;
    if !valid_key(key) {
        return Err(SourceError::InvalidInput("invalid rollout ID".into()));
    }
    let path = path_of(source, key);
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SourceError::Failed("cannot open Codex rollout".into())),
    };
    let before = file
        .metadata()
        .map_err(|_| SourceError::Failed("cannot inspect rollout".into()))?;
    let mut body = String::new();
    file.read_to_string(&mut body)
        .map_err(|_| SourceError::Failed("cannot read rollout".into()))?;
    let after = file
        .metadata()
        .map_err(|_| SourceError::Failed("cannot inspect rollout".into()))?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Ok(None);
    }
    let mut cwd: Option<PathBuf> = None;
    let mut ambiguous = false;
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return Ok(None);
        };
        if !entry.is_object() {
            return Ok(None);
        }
        let value = payload(&entry);
        let kind = value
            .get("type")
            .or_else(|| entry.get("type"))
            .and_then(Value::as_str);
        if matches!(kind, Some("session_meta" | "session")) {
            let next = value
                .get("cwd")
                .and_then(Value::as_str)
                .and_then(absolute_path);
            if next.is_none() || cwd.as_ref().is_some_and(|old| Some(old) != next.as_ref()) {
                ambiguous = true;
            }
            cwd = next;
        }
    }
    if ambiguous {
        cwd = None;
    }
    let raw = RawDocument {
        external_id: key.into(),
        revision: revision_of(&body),
        body,
        metadata: json!({"path":path.display().to_string(), "cwd":cwd}).to_string(),
        occurred_at: None,
    };
    if Codex::normalize(raw.clone())?.segments.is_empty() {
        return Ok(None);
    }
    Ok(Some(raw))
}

fn membership(source: &SourceRef, metadata: &str) -> Option<bool> {
    let Some(project) = project(source) else {
        return Some(true);
    };
    let metadata: Value = serde_json::from_str(metadata).ok()?;
    let cwd = metadata.get("cwd")?.as_str().and_then(absolute_path)?;
    Some(cwd.starts_with(project))
}

impl reconciliation::Guest for Codex {
    fn reconcile(source: SourceRef, known: KnownDocument) -> Result<Decision, SourceError> {
        let raw = snapshot(&source, &known.external_id)?;
        let metadata = raw.as_ref().map(|raw| raw.metadata.as_str());
        // Old membership is usable for scope changes only when the file has actually disappeared.
        let metadata = metadata.or_else(|| {
            (!path_of(&source, &known.external_id).exists()).then_some(known.metadata.as_str())
        });
        match metadata.and_then(|metadata| membership(&source, metadata)) {
            Some(false) => Ok(Decision::Retire("outside_project_scope".into())),
            Some(true) if raw.is_some() => Ok(Decision::Refresh(known.external_id)),
            _ => Ok(Decision::Unchanged),
        }
    }

    fn discover(
        source: SourceRef,
        cursor: String,
        limit: u32,
    ) -> Result<DiscoveryPage, SourceError> {
        let discovery = <Self as Guest>::discover(source.clone(), cursor.clone(), limit)?;
        let cursor_after = Some(
            discovery
                .candidates
                .last()
                .map(|candidate| candidate.cursor_after.clone())
                .unwrap_or_else(|| {
                    if cursor == "null" {
                        json!({"empty":true}).to_string()
                    } else {
                        cursor
                    }
                }),
        );
        let mut candidates = Vec::new();
        for candidate in discovery.candidates {
            // Unknown membership is still registered before acquisition, so incomplete first reads remain recoverable.
            if let Some(raw) = snapshot(&source, &candidate.external_id)?
                && membership(&source, &raw.metadata) == Some(false)
            {
                continue;
            }
            candidates.push(candidate);
        }
        Ok(DiscoveryPage {
            candidates,
            exhausted: discovery.exhausted,
            cursor_after,
        })
    }

    fn acquire(source: SourceRef, candidate: Candidate) -> Result<Acquisition, SourceError> {
        if candidate.handle != candidate.external_id {
            return Err(SourceError::InvalidInput(
                "rollout handle names another document".into(),
            ));
        }
        let Some(raw) = snapshot(&source, &candidate.external_id)? else {
            return Ok(Acquisition::Skip);
        };
        Ok(match membership(&source, &raw.metadata) {
            Some(true) => Acquisition::Document(raw),
            Some(false) => Acquisition::Retire("outside_project_scope".into()),
            None => Acquisition::Skip,
        })
    }
}

impl Guest for Codex {
    fn identify(
        locator: Option<String>,
        options: Vec<(String, String)>,
    ) -> Result<SourceRef, SourceError> {
        let options = match options.as_slice() {
            [] => vec![],
            [(key, value)] if key == "project" => vec![(
                key.clone(),
                absolute_path(value)
                    .ok_or_else(|| {
                        SourceError::InvalidInput("project must be an absolute path".into())
                    })?
                    .display()
                    .to_string(),
            )],
            _ => {
                return Err(SourceError::InvalidInput(
                    "use only project=/absolute/workspace/path".into(),
                ));
            }
        };
        let root = locator.map(PathBuf::from).or_else(default_root).ok_or_else(|| {
            SourceError::InvalidInput("cannot tell where Codex keeps its sessions without a home directory; give the sessions directory as the locator".to_string())
        })?;
        if !root.is_dir() {
            return Err(SourceError::InvalidInput(format!(
                "{} is not a directory of Codex sessions this source can read",
                root.display()
            )));
        }
        Ok(SourceRef {
            source: NAME.to_string(),
            account: None,
            locator: root.display().to_string(),
            options,
        })
    }

    fn default_wing(_source: SourceRef) -> String {
        NAME.to_string()
    }

    fn default_room() -> String {
        "sessions".to_string()
    }

    fn discover(source: SourceRef, cursor: String, limit: u32) -> Result<Discovery, SourceError> {
        let after = parse_cursor(&cursor)?;
        let mut entries = Vec::new();
        collect(Path::new(&source.locator), "", 0, &mut entries);
        entries.sort_by(|a, b| (a.mtime_ns, &a.key).cmp(&(b.mtime_ns, &b.key)));
        entries.retain(|entry| {
            after.as_ref().is_none_or(|(mtime, key)| {
                (entry.mtime_ns, entry.key.as_str()) > (*mtime, key.as_str())
            })
        });
        let exhausted = entries.len() <= limit as usize;
        entries.truncate(limit as usize);
        Ok(Discovery {
            candidates: entries
                .into_iter()
                .map(|entry| Candidate {
                    cursor_after: json!({"mtime_ns": entry.mtime_ns, "key": entry.key}).to_string(),
                    handle: entry.key.clone(),
                    external_id: entry.key,
                })
                .collect(),
            exhausted,
        })
    }

    fn read(source: SourceRef, candidate: Candidate) -> Result<Option<RawDocument>, SourceError> {
        let path = path_of(&source, &candidate.handle);
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(RawDocument {
            external_id: candidate.external_id,
            revision: revision_of(&body),
            body,
            metadata: json!({"path": path.display().to_string()}).to_string(),
            occurred_at: None,
        }))
    }

    fn normalize(raw: RawDocument) -> Result<CanonicalDocument, SourceError> {
        let mut session_id = String::new();
        let mut cwd = None;
        let mut title = None;
        let mut segments = Vec::new();
        for line in raw.body.lines() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let value = payload(&entry);
            let kind = value
                .get("type")
                .or_else(|| entry.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(kind, "session_meta" | "session") {
                session_id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                cwd = value.get("cwd").and_then(Value::as_str).map(str::to_string);
                continue;
            }
            if matches!(kind, "function_call" | "tool_call") {
                let name = value.get("name").and_then(Value::as_str).unwrap_or("tool");
                segments.push(Segment {
                    text: format!("[tool call: {name}]\n\n"),
                });
                continue;
            }
            if kind != "message" {
                continue;
            }
            let role = value
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(role, "user" | "assistant") {
                continue;
            }
            let text = value.get("content").map(text_from).unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            if title.is_none() && role == "user" {
                title = Some(first_line(&text));
            }
            segments.push(Segment {
                text: format!("### {role}\n{text}\n\n"),
            });
        }
        if !segments.is_empty() {
            segments.insert(
                0,
                Segment {
                    text: format!(
                        "# Codex session {session_id}\nworking directory: {}\n\n",
                        cwd.as_deref().unwrap_or("unknown")
                    ),
                },
            );
        }
        let metadata: Value = serde_json::from_str(&raw.metadata).unwrap_or(Value::Null);
        let room = cwd
            .as_deref()
            .and_then(|cwd| Path::new(cwd).file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let name = Path::new(&raw.external_id)
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned());
        Ok(CanonicalDocument {
            title,
            room,
            name,
            kind: SourceKind::Transcript,
            uri: metadata
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string),
            tags: vec!["transcript".to_string(), NAME.to_string()],
            segments,
        })
    }
}

export!(Codex);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_edits_scope_and_incomplete_or_rotated_rollouts() {
        let root = std::env::temp_dir().join(format!("codex-reconcile-{}", std::process::id()));
        let dated = root.join("2026/11/11");
        std::fs::create_dir_all(&dated).unwrap();
        let key = "2026/11/11/rollout-test.jsonl";
        let path = root.join(key);
        let body = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/work/app/sub\"}}\n",
            "{\"type\":\"message\",\"role\":\"user\",\"content\":\"Hello\"}\n"
        );
        std::fs::write(&path, body).unwrap();
        let source = Codex::identify(
            Some(root.display().to_string()),
            vec![("project".into(), "/work/./app".into())],
        )
        .unwrap();
        let candidate = Candidate {
            external_id: key.into(),
            handle: key.into(),
            cursor_after: "null".into(),
        };
        let Acquisition::Document(raw) =
            <Codex as reconciliation::Guest>::acquire(source.clone(), candidate.clone()).unwrap()
        else {
            panic!("expected document")
        };
        let known = KnownDocument {
            pending: false,
            retired: false,
            external_id: key.into(),
            revision: raw.revision.clone(),
            metadata: raw.metadata.clone(),
        };
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(source.clone(), known.clone()).unwrap(),
            Decision::Refresh(_)
        ));
        // A valid shorter rewrite is an edit, whereas a partial trailing record must preserve old content.
        std::fs::write(&path, body.replace("Hello", "Hi")).unwrap();
        let Acquisition::Document(edited) =
            <Codex as reconciliation::Guest>::acquire(source.clone(), candidate.clone()).unwrap()
        else {
            panic!()
        };
        assert_ne!(edited.revision, raw.revision);
        for incomplete in [
            format!("{body}{{"),
            " \n".into(),
            "{}\n".into(),
            "[]\n".into(),
        ] {
            std::fs::write(&path, incomplete).unwrap();
            assert!(matches!(
                <Codex as reconciliation::Guest>::acquire(source.clone(), candidate.clone())
                    .unwrap(),
                Acquisition::Skip
            ));
            assert!(matches!(
                <Codex as reconciliation::Guest>::reconcile(source.clone(), known.clone()).unwrap(),
                Decision::Unchanged
            ));
        }
        std::fs::write(&path, body).unwrap();
        let excluded = SourceRef {
            options: vec![("project".into(), "/work/application".into())],
            ..source.clone()
        };
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(excluded.clone(), known.clone()).unwrap(),
            Decision::Retire(_)
        ));
        assert!(matches!(
            <Codex as reconciliation::Guest>::acquire(excluded.clone(), candidate.clone()).unwrap(),
            Acquisition::Retire(_)
        ));
        let page =
            <Codex as reconciliation::Guest>::discover(excluded.clone(), "null".into(), 1).unwrap();
        assert!(page.candidates.is_empty() && page.exhausted && page.cursor_after.is_some());
        let restored = KnownDocument {
            retired: true,
            ..known.clone()
        };
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(source.clone(), restored).unwrap(),
            Decision::Refresh(_)
        ));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(source.clone(), known.clone()).unwrap(),
            Decision::Unchanged
        ));
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(excluded, known).unwrap(),
            Decision::Retire(_)
        ));
        // Pending registration needs no previous revision or metadata and can recover once the file returns.
        let pending = KnownDocument {
            pending: true,
            retired: false,
            external_id: key.into(),
            revision: String::new(),
            metadata: "{}".into(),
        };
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(source.clone(), pending.clone()).unwrap(),
            Decision::Unchanged
        ));
        std::fs::write(&path, body).unwrap();
        assert!(matches!(
            <Codex as reconciliation::Guest>::reconcile(source.clone(), pending).unwrap(),
            Decision::Refresh(_)
        ));
        assert!(snapshot(&source, "../secret").is_err());
        assert!(
            Codex::identify(
                Some(root.display().to_string()),
                vec![("project".into(), "relative".into())]
            )
            .is_err()
        );
        assert!(!valid_key("/2026/11/11/rollout-test.jsonl"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_project_membership_defers_without_retirement() {
        let root = std::env::temp_dir().join(format!("codex-membership-{}", std::process::id()));
        std::fs::create_dir_all(root.join("2026/11/11")).unwrap();
        let source = Codex::identify(
            Some(root.display().to_string()),
            vec![("project".into(), "/work/app".into())],
        )
        .unwrap();
        let empty =
            <Codex as reconciliation::Guest>::discover(source.clone(), "null".into(), 1).unwrap();
        assert!(empty.exhausted && empty.candidates.is_empty());
        assert!(empty.cursor_after.is_some());
        assert!(
            parse_cursor(empty.cursor_after.as_ref().unwrap())
                .unwrap()
                .is_none()
        );
        let key = "2026/11/11/rollout-test.jsonl";
        for meta in [
            "",
            "{\"type\":\"session_meta\",\"cwd\":\"relative\"}\n",
            "{\"type\":\"session_meta\",\"cwd\":\"/one\"}\n{\"type\":\"session_meta\",\"cwd\":\"/two\"}\n",
        ] {
            std::fs::write(
                root.join(key),
                format!("{meta}{{\"type\":\"message\",\"role\":\"user\",\"content\":\"Hi\"}}"),
            )
            .unwrap();
            let candidate = Candidate {
                external_id: key.into(),
                handle: key.into(),
                cursor_after: "null".into(),
            };
            assert!(matches!(
                <Codex as reconciliation::Guest>::acquire(source.clone(), candidate).unwrap(),
                Acquisition::Skip
            ));
        }
        assert_eq!(
            absolute_path("/work/../app").unwrap(),
            PathBuf::from("/app")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn source(root: &Path) -> SourceRef {
        Codex::identify(Some(root.display().to_string()), vec![]).unwrap()
    }

    #[test]
    fn discovers_only_dated_rollouts_and_reads_the_selected_file() {
        let root = std::env::temp_dir().join(format!("codex-coverage-{}", std::process::id()));
        let dated = root.join("2026/10/18");
        std::fs::create_dir_all(&dated).unwrap();
        std::fs::write(
            dated.join("rollout-a.jsonl"),
            "{\"type\":\"message\",\"role\":\"user\",\"content\":\"Hello\"}\n",
        )
        .unwrap();
        std::fs::write(dated.join("rollout-b.jsonl"), "  \n").unwrap();
        std::fs::write(dated.join("other.jsonl"), "ignored").unwrap();
        std::fs::create_dir_all(dated.join("deeper")).unwrap();
        std::fs::write(dated.join("deeper/rollout-c.jsonl"), "ignored").unwrap();
        let source = source(&root);
        assert_eq!(Codex::default_wing(source.clone()), "codex");
        assert_eq!(Codex::default_room(), "sessions");
        let all = Codex::discover(source.clone(), "null".into(), 10).unwrap();
        assert_eq!(all.candidates.len(), 2);
        assert!(all.exhausted);
        let page = Codex::discover(source.clone(), "null".into(), 1).unwrap();
        assert_eq!(page.candidates.len(), 1);
        assert!(!page.exhausted);
        let next =
            Codex::discover(source.clone(), page.candidates[0].cursor_after.clone(), 10).unwrap();
        assert_eq!(next.candidates.len(), 1);
        assert_ne!(next.candidates[0].handle, page.candidates[0].handle);
        for candidate in all.candidates {
            let raw = Codex::read(source.clone(), candidate.clone()).unwrap();
            if candidate.handle.ends_with("rollout-a.jsonl") {
                let raw = raw.unwrap();
                assert_eq!(raw.revision, revision_of(&raw.body));
                assert_eq!(
                    Codex::normalize(raw).unwrap().title.as_deref(),
                    Some("Hello")
                );
            } else {
                assert!(raw.is_none());
            }
        }
        assert!(
            Codex::read(
                source.clone(),
                Candidate {
                    handle: "2026/10/18/missing.jsonl".into(),
                    external_id: "missing".into(),
                    cursor_after: "null".into(),
                }
            )
            .unwrap()
            .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_bad_input_and_cursors() {
        let root = std::env::temp_dir();
        assert!(matches!(
            Codex::identify(
                Some(root.display().to_string()),
                vec![("unexpected".into(), "x".into())]
            ),
            Err(SourceError::InvalidInput(_))
        ));
        assert!(matches!(
            Codex::identify(
                Some(
                    root.join("nonexistent-codex-sessions")
                        .display()
                        .to_string()
                ),
                vec![]
            ),
            Err(SourceError::InvalidInput(_))
        ));
        let source = source(&root);
        for cursor in [
            "oops",
            "{}",
            r#"{"mtime_ns":1}"#,
            r#"{"mtime_ns":1,"key":"../secret"}"#,
        ] {
            assert!(
                matches!(
                    Codex::discover(source.clone(), cursor.into(), 1),
                    Err(SourceError::CursorInvalid(_))
                ),
                "{cursor}"
            );
        }
        assert!(
            Codex::discover(
                SourceRef {
                    locator: root
                        .join("nonexistent-codex-sessions")
                        .display()
                        .to_string(),
                    ..source
                },
                "null".into(),
                1
            )
            .unwrap()
            .candidates
            .is_empty()
        );
    }

    fn raw(body: &str, metadata: &str) -> RawDocument {
        RawDocument {
            external_id: "2026/10/18/rollout-test.jsonl".into(),
            revision: "r".into(),
            body: body.into(),
            metadata: metadata.into(),
            occurred_at: None,
        }
    }

    #[test]
    fn normalization_preserves_conversation_not_private_or_tool_output() {
        let body = [
            "not json",
            r#"{"type":"session_meta","payload":{"id":"session-1","cwd":"/work/project"}}"#,
            r#"{"type":"message","role":"system","content":"secret instructions"}"#,
            r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"  Prompt  "},{"type":"reasoning","text":"secret reasoning"},{"type":"input_text","text":"Second line"}]}"#,
            r#"{"type":"function_call","name":"search"}"#,
            r#"{"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":" Answer "},{"type":"tool_output","text":"secret result"}]}}"#,
            r#"{"type":"tool_call"}"#,
            r#"{"type":"message","role":"assistant","content":"  "}"#,
            r#"{"type":"reasoning","content":"secret"}"#,
        ].join("\n");
        let result = Codex::normalize(raw(&body, r#"{"path":"/work/session.jsonl"}"#)).unwrap();
        assert_eq!(result.title.as_deref(), Some("Prompt"));
        assert_eq!(result.room.as_deref(), Some("project"));
        assert_eq!(result.name.as_deref(), Some("rollout-test"));
        assert_eq!(result.uri.as_deref(), Some("/work/session.jsonl"));
        let transcript = result
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert!(transcript.contains("Prompt\nSecond line"));
        assert!(transcript.contains("[tool call: search]"));
        assert!(transcript.contains("[tool call: tool]"));
        assert!(transcript.contains("### assistant\nAnswer"));
        assert!(!transcript.contains("secret"));
    }

    #[test]
    fn empty_and_metadata_free_transcripts_are_valid() {
        let empty = Codex::normalize(raw("broken\n{}", "invalid")).unwrap();
        assert!(empty.segments.is_empty());
        assert!(empty.title.is_none());
        assert!(empty.uri.is_none());
        let message = Codex::normalize(raw(
            r#"{"type":"message","role":"assistant","content":"Hello"}"#,
            "{}",
        ))
        .unwrap();
        assert_eq!(message.segments.len(), 2);
        assert!(
            message.segments[0]
                .text
                .contains("working directory: unknown")
        );
    }
}
