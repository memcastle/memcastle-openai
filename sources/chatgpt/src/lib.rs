//! One conversation per document, from a ChatGPT export or the experimental private web interface.
mod export;
mod web;

use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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

struct ChatGpt;

fn failed(message: impl Into<String>) -> SourceError {
    SourceError::Failed(message.into())
}

fn id_of(value: &Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.get("conversation_id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn validate_conversation(value: &Value) -> Result<&str, String> {
    let id = id_of(value).ok_or("conversation has no id")?;
    if let Some(mapping) = value.get("mapping").and_then(Value::as_object) {
        for (node_id, node) in mapping {
            let parent = node.get("parent").unwrap_or(&Value::Null);
            if !node.is_object()
                || !(parent.is_null() || parent.is_string())
                || node
                    .get("message")
                    .is_some_and(|message| !message.is_null() && !message.is_object())
            {
                return Err(format!(
                    "mapping node `{node_id}` has an invalid parent or message"
                ));
            }
        }
    } else if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        if messages.iter().any(|message| !message.is_object()) {
            return Err("conversation messages must be objects".into());
        }
    } else {
        return Err("conversation has neither mapping nor messages".into());
    }
    Ok(id)
}

fn records(source: &SourceRef) -> Result<Vec<Value>, SourceError> {
    if source.locator == "chatgpt:web" {
        let selected = source
            .options
            .iter()
            .find(|(key, _)| key == "projects")
            .map(|(_, value)| {
                let available = web::projects()?;
                web::select_projects(value, &available)
            })
            .transpose()
            .map_err(failed)?;
        return Ok(web::list(selected.as_deref())
            .map_err(failed)?
            .into_iter()
            .map(|listing| {
                json!({
                    "id": listing.id,
                    "project_id": listing.project.as_ref().map(|project| project.id.as_str()),
                    "project_name": listing.project.as_ref().map(|project| project.name.as_str()),
                })
            })
            .collect());
    }
    export::ids(&PathBuf::from(&source.locator)).map_err(failed)
}

fn cursor(cursor: &str) -> Result<(Option<String>, bool), SourceError> {
    let value: Value = serde_json::from_str(cursor)
        .map_err(|_| SourceError::CursorInvalid("the cursor is not JSON".into()))?;
    if value.is_null() {
        return Ok((None, false));
    }
    if value.get("after") == Some(&Value::Null) && value.get("done") == Some(&Value::Bool(true)) {
        return Ok((None, true));
    }
    let key = value
        .get("after")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SourceError::CursorInvalid("the cursor has no conversation ID".into()))?;
    let done = value
        .get("done")
        .and_then(Value::as_bool)
        .ok_or_else(|| SourceError::CursorInvalid("the cursor has no sweep marker".into()))?;
    Ok((Some(key.to_string()), done))
}

fn revision(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}

fn timestamp(value: &Value) -> Option<String> {
    let value = value.get("create_time")?;
    // Export times are Unix timestamps; web times may already be RFC 3339-like strings.
    if let Some(time) = value.as_str() {
        return Some(time.to_string());
    }
    let seconds = value.as_f64()?;
    let whole = seconds.floor() as i64;
    let nanos = ((seconds - whole as f64) * 1e9) as u32;
    // Avoid adding a date-time dependency just to format the source's own timestamp.
    let date = time::OffsetDateTime::from_unix_timestamp(whole)
        .ok()?
        .replace_nanosecond(nanos)
        .ok()?;
    date.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn content_text(content: &Value) -> String {
    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        return String::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            part.as_str()
                .or_else(|| part.get("text").and_then(Value::as_str))
        })
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn segments(value: &Value) -> Result<Vec<Segment>, SourceError> {
    let mut entries: Vec<(String, Option<String>, &Value)> = Vec::new();
    if let Some(mapping) = value.get("mapping").and_then(Value::as_object) {
        // Walk parent relationships rather than relying on JSON object order, which differs across exports.
        let mut keys: Vec<_> = mapping.keys().collect();
        keys.sort();
        for key in keys {
            let node = &mapping[key];
            if let Some(message) = node.get("message").filter(|m| !m.is_null()) {
                let parent = node
                    .get("parent")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                entries.push((key.to_string(), parent, message));
            }
        }
        // Ordering by graph depth and ID gives stable parent-before-child output, including divergent branches.
        entries.sort_by_key(|(id, _, _)| {
            let mut depth = 0usize;
            let mut next = id.as_str();
            while depth <= mapping.len() {
                let Some(parent) = mapping
                    .get(next)
                    .and_then(|node| node.get("parent"))
                    .and_then(Value::as_str)
                else {
                    break;
                };
                depth += 1;
                next = parent;
            }
            (depth, id.clone())
        });
    } else if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for (index, message) in messages.iter().enumerate() {
            let id = message
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| index.to_string());
            entries.push((
                id,
                message
                    .get("parent")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                message,
            ));
        }
    } else {
        return Err(failed("conversation has neither mapping nor messages"));
    }

    let mut out = Vec::new();
    for (id, parent, message) in entries {
        let author = message
            .get("author")
            .and_then(|a| a.get("role"))
            .and_then(Value::as_str)
            .or_else(|| message.get("role").and_then(Value::as_str))
            .unwrap_or("unknown");
        let channel = message.get("channel").and_then(Value::as_str).or_else(|| {
            message
                .get("metadata")
                .and_then(|m| m.get("channel"))
                .and_then(Value::as_str)
        });
        // Internal reasoning and tool output are not conversation turns; raw records still retain source metadata.
        if !matches!(author, "user" | "assistant") || channel.is_some_and(|c| c != "final") {
            continue;
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        let text = content_text(content);
        if text.is_empty() {
            continue;
        }
        let model = message
            .get("metadata")
            .and_then(|m| m.get("model_slug"))
            .and_then(Value::as_str)
            .map(|m| format!(" model={m}"))
            .unwrap_or_default();
        let name = message
            .get("author")
            .and_then(|author| author.get("name"))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(|name| format!(" name={name}"))
            .unwrap_or_default();
        let time = timestamp(message)
            .map(|t| format!(" at {t}"))
            .unwrap_or_default();
        let parent = parent.map(|p| format!(" parent={p}")).unwrap_or_default();
        out.push(Segment {
            text: format!("[{author} id={id}{parent}{name}{model}{time}]\n{text}\n\n"),
        });
    }
    Ok(out)
}

impl Guest for ChatGpt {
    fn identify(
        locator: Option<String>,
        options: Vec<(String, String)>,
    ) -> Result<SourceRef, SourceError> {
        // Accept our canonical web locator as well as the locator-less shorthand.
        let locator = locator.filter(|locator| locator != "chatgpt:web");
        let mut account = None;
        let mut mode = None;
        let mut projects = None;
        for (key, value) in &options {
            if key == "mode" && mode.is_none() && (value == "web" || value == "export") {
                mode = Some(value.clone());
            } else if key == "account"
                && account.is_none()
                && !value.is_empty()
                && value.len() <= 80
            {
                account = Some(value.clone());
            } else if key == "projects" && projects.is_none() {
                projects = Some(value.clone());
            } else {
                return Err(SourceError::InvalidInput("use mode=web [projects=id:ID,name:NAME] for the signed-in account, or an export ZIP/JSON path [mode=export]".into()));
            }
        }
        // A locator names an offline export, which does not need a web sign-in. In its absence the sole registered
        // web account is selected automatically; named account selection is not supported by the current auth grant.
        let web_mode =
            match (mode.as_deref(), locator.as_deref()) {
                (Some("web"), Some(_)) => return Err(SourceError::InvalidInput(
                    "mode=web conflicts with an export path; remove the path or select mode=export"
                        .into(),
                )),
                (Some("export"), None) => {
                    return Err(SourceError::InvalidInput(
                        "mode=export needs an export ZIP or conversations.json path".into(),
                    ));
                }
                (Some("web"), None) | (None, None) => true,
                (Some("export"), Some(_)) | (None, Some(_)) => false,
                _ => unreachable!("the parser accepted only web or export"),
            };
        if web_mode {
            if locator.is_some() {
                return Err(SourceError::InvalidInput(
                    "mode=web does not take an export file path".into(),
                ));
            }
            // A web run must establish its authorization before the pipeline creates its source record or cursor.
            web::check_auth().map_err(failed)?;
            let account = account.unwrap_or_else(|| "default".to_string());
            let mut options = options;
            if let Some(selection) = projects {
                let available = web::projects().map_err(failed)?;
                let selected = web::select_projects(&selection, &available)
                    .map_err(SourceError::InvalidInput)?;
                let ids: Vec<_> = selected.iter().map(|project| project.id.as_str()).collect();
                // Normalizing names into IDs pins this run's discovery to the exact slice it identified.
                if let Some((_, value)) = options.iter_mut().find(|(key, _)| key == "projects") {
                    *value = ids
                        .iter()
                        .map(|id| format!("id:{id}"))
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
            options.sort();
            return Ok(SourceRef {
                source: "chatgpt".into(),
                account: Some(account),
                locator: "chatgpt:web".into(),
                options,
            });
        }
        if account.is_some() || projects.is_some() {
            return Err(SourceError::InvalidInput(
                "account=LABEL and projects=… require mode=web; export files cannot be filtered by project".into(),
            ));
        }
        let locator = locator.expect("the export branch has a locator");
        // The host canonicalizes a granted locator before opening its parent; WASI cannot resolve ancestors
        // outside that preopen a second time.
        let path = PathBuf::from(&locator);
        if !path.is_file() {
            return Err(SourceError::InvalidInput(
                "the export locator must be a file".into(),
            ));
        }
        // Check before creating a source record or a cursor, so an unrelated ZIP never becomes a successful run.
        export::ids(&path).map_err(SourceError::InvalidInput)?;
        Ok(SourceRef {
            source: "chatgpt".into(),
            account: None,
            locator: path.to_string_lossy().into_owned(),
            options: vec![],
        })
    }

    fn default_wing(_source: SourceRef) -> String {
        "chatgpt".into()
    }
    fn default_room() -> String {
        "conversations".into()
    }

    fn discover(source: SourceRef, previous: String, limit: u32) -> Result<Discovery, SourceError> {
        let (after, done) = cursor(&previous)?;
        let mut items: Vec<_> = records(&source)?
            .into_iter()
            .map(|value| {
                id_of(&value)
                    .map(|id| (id.to_string(), value.clone()))
                    .ok_or_else(|| failed("a ChatGPT conversation has no id"))
            })
            .collect::<Result<_, _>>()?;
        items.sort_by(|a, b| a.0.cmp(&b.0));
        if items.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(failed("the ChatGPT listing has duplicate conversation IDs"));
        }
        let mut remaining: Vec<_> = items
            .iter()
            .filter(|(id, _)| done || after.as_ref().is_none_or(|prev| id > prev))
            .cloned()
            .collect();
        // The last seen conversation may have been deleted since the previous run. Restart the sweep rather than
        // leaving a cursor beyond every remaining ID forever.
        if remaining.is_empty() && after.is_some() && !done {
            remaining = items;
        }
        let count = remaining.len().min(limit as usize);
        let exhausted = count == remaining.len();
        let candidates = remaining
            .into_iter()
            .take(count)
            .enumerate()
            .map(|(index, (id, value))| Candidate {
                cursor_after: json!({"after": id, "done": exhausted && index + 1 == count})
                    .to_string(),
                handle: if source.locator == "chatgpt:web" {
                    value.to_string()
                } else {
                    id.clone()
                },
                external_id: id,
            })
            .collect();
        Ok(Discovery {
            candidates,
            exhausted,
        })
    }

    fn read(source: SourceRef, candidate: Candidate) -> Result<Option<RawDocument>, SourceError> {
        let mut project = None;
        let value = if source.locator == "chatgpt:web" {
            let handle: Value = serde_json::from_str(&candidate.handle)
                .map_err(|_| failed("ChatGPT conversation handle is invalid"))?;
            if id_of(&handle) != Some(candidate.external_id.as_str()) {
                return Err(failed("ChatGPT conversation handle names another document"));
            }
            project = match (
                handle.get("project_id").and_then(Value::as_str),
                handle.get("project_name").and_then(Value::as_str),
            ) {
                (Some(id), Some(name)) => Some((id.to_string(), name.to_string())),
                (None, None) => None,
                _ => return Err(failed("ChatGPT project provenance is incomplete")),
            };
            let value = web::conversation(&candidate.external_id).map_err(failed)?;
            if let Some(current) = value.get("gizmo_id").and_then(Value::as_str)
                && project.as_ref().is_none_or(|(id, _)| id != current)
            {
                return Err(failed(
                    "ChatGPT project membership changed during acquisition; retry the mine",
                ));
            }
            value
        } else {
            let Some(value) =
                export::conversation(&PathBuf::from(&source.locator), &candidate.handle)
                    .map_err(failed)?
            else {
                return Ok(None);
            };
            value
        };
        if id_of(&value) != Some(candidate.external_id.as_str()) {
            return Err(failed("ChatGPT returned a different conversation ID"));
        }
        let body =
            serde_json::to_string(&value).map_err(|_| failed("cannot encode the conversation"))?;
        if body.len() > export::MAX_CONVERSATION {
            return Err(failed(
                "conversation exceeds 16 MiB; use an export split into smaller files",
            ));
        }
        let occurred_at = timestamp(&value);
        Ok(Some(RawDocument {
            external_id: candidate.external_id,
            revision: revision(&body),
            body,
            metadata: if source.locator == "chatgpt:web" {
                json!({"backend": "web", "project_id": project.as_ref().map(|p| &p.0),
                       "project_name": project.as_ref().map(|p| &p.1)})
                .to_string()
            } else {
                json!({"backend": "export"}).to_string()
            },
            occurred_at,
        }))
    }

    fn normalize(raw: RawDocument) -> Result<CanonicalDocument, SourceError> {
        let value: Value = serde_json::from_str(&raw.body)
            .map_err(|_| failed("malformed ChatGPT conversation"))?;
        validate_conversation(&value).map_err(failed)?;
        let id = id_of(&value).unwrap_or_default();
        Ok(CanonicalDocument {
            title: value
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            room: None,
            name: None,
            kind: SourceKind::Transcript,
            uri: Some(format!("https://chatgpt.com/c/{id}")),
            tags: vec!["chatgpt".into(), "transcript".into()],
            segments: segments(&value)?,
        })
    }
}

impl reconciliation::Guest for ChatGpt {
    fn reconcile(source: SourceRef, known: KnownDocument) -> Result<Decision, SourceError> {
        // Core stores IDs before first acquisition; neither a saved discovery handle nor metadata is required.
        if source.locator != "chatgpt:web" {
            return Ok(
                if export::conversation(&PathBuf::from(&source.locator), &known.external_id)
                    .map_err(failed)?
                    .is_some()
                {
                    Decision::Refresh(known.external_id)
                } else {
                    Decision::Unchanged
                },
            );
        }
        Ok(Decision::Refresh(
            json!({"id":known.external_id}).to_string(),
        ))
    }

    fn discover(
        source: SourceRef,
        previous: String,
        limit: u32,
    ) -> Result<DiscoveryPage, SourceError> {
        let (after, done) = cursor(&previous)?;
        let mut items = records(&source)?;
        items.sort_by(|a, b| id_of(a).cmp(&id_of(b)));
        if items
            .windows(2)
            .any(|pair| id_of(&pair[0]) == id_of(&pair[1]))
        {
            return Err(failed("the ChatGPT listing has duplicate conversation IDs"));
        }
        let remaining: Vec<_> = items
            .into_iter()
            .filter(|value| {
                done || after
                    .as_deref()
                    .is_none_or(|after| id_of(value).is_some_and(|id| id > after))
            })
            .collect();
        let exhausted = remaining.len() <= limit as usize;
        let mut candidates = Vec::new();
        for value in remaining.into_iter().take(limit as usize) {
            let id = id_of(&value)
                .ok_or_else(|| failed("a ChatGPT conversation has no id"))?
                .to_string();
            candidates.push(Candidate {
                cursor_after: json!({"after":id,"done":false}).to_string(),
                handle: if source.locator == "chatgpt:web" {
                    value.to_string()
                } else {
                    id.clone()
                },
                external_id: id,
            });
        }
        let cursor_after = if exhausted {
            Some(json!({"after":candidates.last().map(|candidate| &candidate.external_id).or(after.as_ref()),"done":true}).to_string())
        } else {
            candidates
                .last()
                .map(|candidate| candidate.cursor_after.clone())
        };
        Ok(DiscoveryPage {
            candidates,
            exhausted,
            cursor_after,
        })
    }

    fn acquire(source: SourceRef, mut candidate: Candidate) -> Result<Acquisition, SourceError> {
        if source.locator == "chatgpt:web" {
            let handle: Value = serde_json::from_str(&candidate.handle)
                .map_err(|_| failed("invalid ChatGPT handle"))?;
            if id_of(&handle) != Some(candidate.external_id.as_str()) {
                return Err(failed("ChatGPT handle names another document"));
            }
            // Positive current membership is evidence; absence from this listing never proves removal.
            let available = web::projects().map_err(failed)?;
            let selection = source
                .options
                .iter()
                .find(|(key, _)| key == "projects")
                .map(|(_, value)| web::select_projects(value, &available))
                .transpose()
                .map_err(failed)?;
            let listing = web::list(Some(&available))
                .map_err(failed)?
                .into_iter()
                .find(|entry| entry.id == candidate.external_id);
            if let Some(selected) = &selection {
                match listing.as_ref().and_then(|entry| entry.project.as_ref()) {
                    Some(project) if !selected.iter().any(|selected| selected.id == project.id) => {
                        return Ok(Acquisition::Retire("outside_project_scope".into()));
                    }
                    Some(_) => (),
                    None => return Ok(Acquisition::Skip),
                }
            }
            let project = listing.and_then(|entry| entry.project);
            candidate.handle = json!({"id":candidate.external_id,"project_id":project.as_ref().map(|p| &p.id),"project_name":project.as_ref().map(|p| &p.name)}).to_string();
        } else if candidate.handle != candidate.external_id {
            return Err(failed("export handle names another document"));
        }
        Ok(match <Self as Guest>::read(source, candidate)? {
            Some(raw) => Acquisition::Document(raw),
            None => Acquisition::Skip,
        })
    }
}

#[cfg(target_arch = "wasm32")]
export!(ChatGpt);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_reconciliation_preserves_omissions_and_recovers_pending_ids() {
        let path =
            std::env::temp_dir().join(format!("chatgpt-reconcile-{}.json", std::process::id()));
        std::fs::write(&path, r#"[{"id":"a","messages":[]}]"#).unwrap();
        let source = ChatGpt::identify(Some(path.display().to_string()), vec![]).unwrap();
        let known = KnownDocument {
            pending: true,
            retired: false,
            external_id: "a".into(),
            revision: String::new(),
            metadata: "{}".into(),
        };
        assert!(matches!(
            <ChatGpt as reconciliation::Guest>::reconcile(source.clone(), known.clone()).unwrap(),
            Decision::Refresh(_)
        ));
        let candidate = Candidate {
            external_id: "a".into(),
            handle: "a".into(),
            cursor_after: "null".into(),
        };
        assert!(matches!(
            <ChatGpt as reconciliation::Guest>::acquire(source.clone(), candidate.clone()).unwrap(),
            Acquisition::Document(_)
        ));
        let first =
            <ChatGpt as reconciliation::Guest>::discover(source.clone(), "null".into(), 1).unwrap();
        assert_eq!(first.candidates.len(), 1);
        std::fs::write(&path, "[]").unwrap();
        assert!(matches!(
            <ChatGpt as reconciliation::Guest>::reconcile(source.clone(), known.clone()).unwrap(),
            Decision::Unchanged
        ));
        assert!(matches!(
            <ChatGpt as reconciliation::Guest>::acquire(source.clone(), candidate).unwrap(),
            Acquisition::Skip
        ));
        let empty = <ChatGpt as reconciliation::Guest>::discover(
            source.clone(),
            first.candidates[0].cursor_after.clone(),
            1,
        )
        .unwrap();
        assert!(empty.candidates.is_empty() && empty.exhausted);
        assert!(empty.cursor_after.is_some());
        let again = <ChatGpt as reconciliation::Guest>::discover(
            source.clone(),
            empty.cursor_after.unwrap(),
            1,
        )
        .unwrap();
        assert!(again.candidates.is_empty() && again.exhausted);
        std::fs::write(&path, "[").unwrap();
        assert!(
            <ChatGpt as reconciliation::Guest>::reconcile(source.clone(), known.clone()).is_err()
        );
        std::fs::remove_file(path).unwrap();
        assert!(<ChatGpt as reconciliation::Guest>::reconcile(source, known).is_err());
    }

    #[test]
    fn web_scope_uses_current_positive_membership_and_restores_unchanged_ids() {
        web::mocked(
            |path| {
                Ok(if path.contains("sidebar") {
                    json!({"items":[{"gizmo":{"gizmo":{"id":"one","display":{"name":"Alpha"}}}},{"gizmo":{"gizmo":{"id":"two","display":{"name":"Beta"}}}}]})
                } else if path.contains("gizmos/one/") {
                    json!({"items":[]})
                } else if path.contains("gizmos/two/") {
                    json!({"items":[{"id":"chat","gizmo_id":"two"}]})
                } else {
                    json!({"messages":[],"page_info":{"has_previous_page":false}})
                })
            },
            || {
                let source = SourceRef {
                    source: "chatgpt".into(),
                    account: Some("default".into()),
                    locator: "chatgpt:web".into(),
                    options: vec![("projects".into(), "id:one".into())],
                };
                let known = KnownDocument {
                    pending: false,
                    retired: true,
                    external_id: "chat".into(),
                    revision: "same".into(),
                    metadata: "{}".into(),
                };
                let Decision::Refresh(handle) =
                    <ChatGpt as reconciliation::Guest>::reconcile(source.clone(), known).unwrap()
                else {
                    panic!()
                };
                let candidate = Candidate {
                    external_id: "chat".into(),
                    handle,
                    cursor_after: "null".into(),
                };
                assert!(matches!(
                    <ChatGpt as reconciliation::Guest>::acquire(source.clone(), candidate.clone())
                        .unwrap(),
                    Acquisition::Retire(_)
                ));
                let restored = SourceRef {
                    options: vec![("projects".into(), "id:two".into())],
                    ..source.clone()
                };
                let Acquisition::Document(raw) =
                    <ChatGpt as reconciliation::Guest>::acquire(restored, candidate.clone())
                        .unwrap()
                else {
                    panic!()
                };
                assert!(raw.metadata.contains("Beta"));
                let unknown = Candidate {
                    external_id: "unknown".into(),
                    handle: json!({"id":"unknown"}).to_string(),
                    ..candidate
                };
                assert!(matches!(
                    <ChatGpt as reconciliation::Guest>::acquire(source, unknown).unwrap(),
                    Acquisition::Skip
                ));
            },
        );
        web::mocked(
            |_| Err("global credential failure".into()),
            || {
                let source = SourceRef {
                    source: "chatgpt".into(),
                    account: None,
                    locator: "chatgpt:web".into(),
                    options: vec![],
                };
                let candidate = Candidate {
                    external_id: "chat".into(),
                    handle: json!({"id":"chat"}).to_string(),
                    cursor_after: "null".into(),
                };
                assert!(<ChatGpt as reconciliation::Guest>::acquire(source, candidate).is_err());
            },
        );
    }

    #[test]
    fn contradictory_detail_membership_cannot_replace_a_transcript() {
        web::mocked(
            |path| {
                Ok(if path.contains("sidebar") {
                    json!({"items":[{"gizmo":{"gizmo":{"id":"one","display":{"name":"Alpha"}}}}]})
                } else if path.contains("gizmos/one/") {
                    json!({"items":[{"id":"chat","gizmo_id":"one"}]})
                } else {
                    json!({"gizmo_id":"other","messages":[],"page_info":{"has_previous_page":false}})
                })
            },
            || {
                let source = SourceRef {
                    source: "chatgpt".into(),
                    account: None,
                    locator: "chatgpt:web".into(),
                    options: vec![],
                };
                let candidate = Candidate {
                    external_id: "chat".into(),
                    handle: json!({"id":"chat"}).to_string(),
                    cursor_after: "null".into(),
                };
                assert!(
                    matches!(<ChatGpt as reconciliation::Guest>::acquire(source, candidate), Err(SourceError::Failed(message)) if message.contains("membership changed"))
                );
            },
        );
    }

    fn fixture() -> String {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/export/conversations.json"
        )
        .into()
    }

    fn source() -> SourceRef {
        ChatGpt::identify(Some(fixture()), vec![]).unwrap()
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            external_id: id.into(),
            handle: id.into(),
            cursor_after: "null".into(),
        }
    }

    #[test]
    fn export_is_discovered_read_and_normalized_in_pages() {
        let source = source();
        assert_eq!(ChatGpt::default_wing(source.clone()), "chatgpt");
        assert_eq!(ChatGpt::default_room(), "conversations");
        let all = ChatGpt::discover(source.clone(), "null".into(), 100).unwrap();
        assert!(all.exhausted);
        assert!(all.candidates.len() >= 2);
        let first = ChatGpt::discover(source.clone(), "null".into(), 1).unwrap();
        assert!(!first.exhausted);
        let next = ChatGpt::discover(
            source.clone(),
            first.candidates[0].cursor_after.clone(),
            100,
        )
        .unwrap();
        assert_eq!(
            first.candidates.len() + next.candidates.len(),
            all.candidates.len()
        );
        assert_eq!(
            serde_json::from_str::<Value>(&next.candidates.last().unwrap().cursor_after).unwrap()["done"],
            true
        );
        for candidate in all.candidates {
            let raw = ChatGpt::read(source.clone(), candidate.clone())
                .unwrap()
                .unwrap();
            assert_eq!(raw.revision, revision(&raw.body));
            assert_eq!(
                serde_json::from_str::<Value>(&raw.metadata).unwrap()["backend"],
                "export"
            );
            let document = ChatGpt::normalize(raw).unwrap();
            assert_eq!(
                document.uri.as_deref(),
                Some(format!("https://chatgpt.com/c/{}", candidate.external_id).as_str())
            );
            assert!(!document.segments.is_empty());
        }
        assert!(
            ChatGpt::read(source, candidate("missing"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bad_locators_options_and_cursors_are_rejected() {
        assert!(matches!(
            ChatGpt::identify(Some("missing-export".into()), vec![]),
            Err(SourceError::InvalidInput(_))
        ));
        for options in [
            vec![("mode".into(), "unknown".into())],
            vec![("account".into(), "me".into())],
            vec![("projects".into(), "id:one".into())],
        ] {
            assert!(matches!(
                ChatGpt::identify(Some(fixture()), options),
                Err(SourceError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            ChatGpt::identify(None, vec![("mode".into(), "export".into())]),
            Err(SourceError::InvalidInput(_))
        ));
        assert!(ChatGpt::identify(Some(fixture()), vec![("mode".into(), "export".into())]).is_ok());
        assert!(matches!(
            ChatGpt::identify(
                Some(fixture()),
                vec![
                    ("mode".into(), "web".into()),
                    ("account".into(), "me".into())
                ]
            ),
            Err(SourceError::InvalidInput(_))
        ));
        for cursor in [
            "broken",
            "{}",
            r#"{"after":"a"}"#,
            r#"{"after":"","done":false}"#,
            r#"{"after":"a","done":1}"#,
        ] {
            assert!(matches!(
                ChatGpt::discover(source(), cursor.into(), 1),
                Err(SourceError::CursorInvalid(_))
            ));
        }
    }

    #[test]
    fn sweep_restarts_after_deletion_and_rejects_duplicates() {
        let root = std::env::temp_dir().join(format!("chatgpt-coverage-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("conversations.json");
        std::fs::write(
            &file,
            r#"[{"id":"a","messages":[]},{"id":"b","messages":[]}]"#,
        )
        .unwrap();
        let source = ChatGpt::identify(Some(file.display().to_string()), vec![]).unwrap();
        let complete = ChatGpt::discover(source.clone(), "null".into(), 10).unwrap();
        assert_eq!(complete.candidates.len(), 2);
        let repeat = ChatGpt::discover(
            source.clone(),
            complete.candidates[1].cursor_after.clone(),
            10,
        )
        .unwrap();
        assert_eq!(repeat.candidates.len(), 2);
        std::fs::write(&file, r#"[{"id":"a","messages":[]}]"#).unwrap();
        let restarted =
            ChatGpt::discover(source.clone(), r#"{"after":"b","done":false}"#.into(), 10).unwrap();
        assert_eq!(restarted.candidates[0].external_id, "a");
        std::fs::write(
            &file,
            r#"[{"id":"a","messages":[]},{"id":"a","messages":[]}]"#,
        )
        .unwrap();
        assert!(matches!(
            ChatGpt::discover(source.clone(), "null".into(), 10),
            Err(SourceError::Failed(_))
        ));
        assert!(matches!(
            ChatGpt::read(source, candidate("a")),
            Err(SourceError::Failed(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn normalization_filters_nonfinal_messages_and_invalid_records() {
        let value = json!({"id":"chat","title":"Title","messages":[
            {"id":"u","role":"user","content":{"parts":[" Hello ",{"text":"World"}]}},
            {"id":"r","role":"assistant","channel":"analysis","content":{"parts":["private"]}},
            {"id":"t","role":"tool","content":{"parts":["output"]}},
            {"id":"a","author":{"role":"assistant","name":"bot"},"metadata":{"model_slug":"model","channel":"final"},"content":{"parts":["Reply"]},"create_time":1767225600}
        ]});
        let raw = RawDocument {
            external_id: "chat".into(),
            revision: "rev".into(),
            body: value.to_string(),
            metadata: "{}".into(),
            occurred_at: None,
        };
        let doc = ChatGpt::normalize(raw.clone()).unwrap();
        assert_eq!(doc.title.as_deref(), Some("Title"));
        assert_eq!(doc.segments.len(), 2);
        assert!(
            doc.segments[1]
                .text
                .contains("name=bot model=model at 2026-01-01T00:00:00Z")
        );
        assert!(
            !doc.segments
                .iter()
                .any(|s| s.text.contains("private") || s.text.contains("output"))
        );
        assert!(matches!(
            ChatGpt::normalize(RawDocument {
                body: "invalid".into(),
                ..raw.clone()
            }),
            Err(SourceError::Failed(_))
        ));
        assert!(matches!(
            ChatGpt::normalize(RawDocument {
                body: json!({"id":"chat"}).to_string(),
                ..raw
            }),
            Err(SourceError::Failed(_))
        ));
        assert_eq!(
            timestamp(&json!({"create_time":"2026-01-01"})).as_deref(),
            Some("2026-01-01")
        );
        assert!(timestamp(&json!({"create_time":"not-a-number"})).is_some());
        assert!(timestamp(&json!({"create_time":null})).is_none());
    }

    #[test]
    fn rejects_mismatched_ids_and_invalid_web_handles_before_network() {
        let mut mismatched = candidate("different");
        mismatched.handle = "conversation-a".into();
        assert!(matches!(
            ChatGpt::read(source(), mismatched),
            Err(SourceError::Failed(_))
        ));
        let web = SourceRef {
            locator: "chatgpt:web".into(),
            ..source()
        };
        for handle in [
            "oops",
            r#"{"id":"different"}"#,
            r#"{"id":"same","project_id":"one"}"#,
        ] {
            assert!(matches!(
                ChatGpt::read(
                    web.clone(),
                    Candidate {
                        external_id: "same".into(),
                        handle: handle.into(),
                        cursor_after: "null".into()
                    }
                ),
                Err(SourceError::Failed(_))
            ));
        }
    }
}
