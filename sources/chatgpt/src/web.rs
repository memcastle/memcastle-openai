use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};

use crate::memcastle::source::host::{access_token, run_process};

#[cfg(test)]
thread_local! {
    static MOCK_REQUEST: std::cell::RefCell<Option<Box<dyn Fn(&str) -> Result<Value, String>>>> =
        std::cell::RefCell::new(None);
}

const PAGE: usize = 20;
const MAX_LIST_PAGES: usize = 500;
const MAX_MESSAGE_PAGES: usize = 500;
const MAX_PROJECT_PAGES: usize = 100;
// A plain curl user agent received an HTML edge challenge even with a freshly issued OAuth token.
// This browser-style value received a JSON 200 without any cookie, including after token refresh.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub(super) fn check_auth() -> Result<(), String> {
    bearer().map(|_| ())
}

fn bearer() -> Result<String, String> {
    if let Ok(token) = std::env::var("MEMCASTLE_CHATGPT_BEARER")
        && !token.is_empty()
    {
        return Ok(token);
    }
    access_token().map_err(|_| {
        "sign in with `memcastle source auth chatgpt` before mining the web source".into()
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug)]
pub(super) struct Listing {
    pub id: String,
    pub project: Option<Project>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn next_cursor(response: &Value) -> Result<Option<&str>, String> {
    match response.get("cursor") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cursor)) if !cursor.is_empty() && cursor.len() <= 2048 => {
            Ok(Some(cursor))
        }
        _ => Err("ChatGPT returned an invalid project-page cursor".into()),
    }
}

pub(super) fn projects() -> Result<Vec<Project>, String> {
    let mut result = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PROJECT_PAGES {
        let mut path = format!("/backend-api/gizmos/snorlax/sidebar?limit={PAGE}");
        if let Some(cursor) = &cursor {
            path.push_str(&format!("&cursor={}", encode(cursor)));
        }
        let response = request(&path)?;
        let items = response
            .get("items")
            .and_then(Value::as_array)
            .ok_or("ChatGPT project sidebar has no items")?;
        if items.len() > PAGE {
            return Err("ChatGPT returned an oversized project sidebar page".into());
        }
        for item in items {
            let project = &item["gizmo"]["gizmo"];
            let id = project
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_id(id))
                .ok_or("a ChatGPT project has no valid ID")?;
            let name = project["display"]["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or("a ChatGPT project has no display name")?;
            if !ids.insert(id.to_string()) {
                return Err("ChatGPT project sidebar repeated a project; retry the mine".into());
            }
            result.push(Project {
                id: id.to_string(),
                name: name.to_string(),
            });
        }
        let Some(next) = next_cursor(&response)? else {
            return Ok(result);
        };
        if items.is_empty() || !cursors.insert(next.to_string()) {
            return Err("ChatGPT project sidebar repeated a cursor or ended prematurely".into());
        }
        cursor = Some(next.to_string());
    }
    Err("ChatGPT project sidebar exceeds its paging limit; refusing a partial listing".into())
}

pub(super) fn select_projects(
    selection: &str,
    available: &[Project],
) -> Result<Vec<Project>, String> {
    let mut selected = BTreeMap::new();
    if selection.is_empty() {
        return Err("projects is empty; omit it to mine everything".into());
    }
    for selector in selection.split(',') {
        let project = if let Some(id) = selector.strip_prefix("id:") {
            if !valid_id(id) {
                return Err("projects=id:… needs a valid ChatGPT project ID".into());
            }
            available
                .iter()
                .find(|project| project.id == id)
                .ok_or("the selected ChatGPT project ID is unavailable; check its ID and account")?
        } else if let Some(name) = selector.strip_prefix("name:") {
            if name.is_empty() {
                return Err("projects=name:… needs a nonempty display name".into());
            }
            let mut matches = available.iter().filter(|project| project.name == name);
            let first = matches.next().ok_or("the named ChatGPT project was not found; a renamed project needs its new name or ID")?;
            if matches.next().is_some() {
                return Err(
                    "more than one ChatGPT project has that name; use projects=id:…".into(),
                );
            }
            first
        } else {
            return Err(
                "projects accepts comma-separated id:PROJECT_ID or name:EXACT_NAME values".into(),
            );
        };
        if selected
            .insert(project.id.clone(), project.clone())
            .is_some()
        {
            return Err("the same ChatGPT project is selected twice".into());
        }
    }
    Ok(selected.into_values().collect())
}

fn project_conversations(project: &Project) -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let mut path = format!(
            "/backend-api/gizmos/{}/conversations?limit={PAGE}",
            encode(&project.id)
        );
        if let Some(cursor) = &cursor {
            path.push_str(&format!("&cursor={}", encode(cursor)));
        }
        let response = request(&path)?;
        let items = response
            .get("items")
            .and_then(Value::as_array)
            .ok_or("ChatGPT project conversations have no items")?;
        if items.len() > PAGE {
            return Err("ChatGPT returned an oversized project conversation page".into());
        }
        for item in items {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_id(id))
                .ok_or("a ChatGPT project conversation has no valid ID")?;
            if item.get("gizmo_id").and_then(Value::as_str) != Some(project.id.as_str()) {
                return Err(
                    "a ChatGPT conversation belongs to another project; retry the mine".into(),
                );
            }
            if !ids.insert(id.to_string()) {
                return Err(
                    "ChatGPT project conversation pages repeated an ID; retry the mine".into(),
                );
            }
            listings.push(Listing {
                id: id.to_string(),
                project: Some(project.clone()),
            });
        }
        let Some(next) = next_cursor(&response)? else {
            return Ok(listings);
        };
        if items.is_empty() || !cursors.insert(next.to_string()) {
            return Err(
                "ChatGPT project conversations repeated a cursor or ended prematurely".into(),
            );
        }
        cursor = Some(next.to_string());
    }
    Err("ChatGPT project conversations exceed the page limit; refusing partial history".into())
}

fn escape(value: &str) -> Result<String, String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err("a web session header contains a line break".into());
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn request(path: &str) -> Result<Value, String> {
    #[cfg(test)]
    if let Some(result) = MOCK_REQUEST.with(|mock| mock.borrow().as_ref().map(|mock| mock(path))) {
        return result;
    }
    let bearer = escape(&bearer()?)?;
    if bearer.is_empty() {
        return Err("the ChatGPT bearer session is empty".into());
    }
    let mut config = format!(
        "url = \"https://chatgpt.com{path}\"\nheader = \"Authorization: Bearer {bearer}\"\nheader = \"Accept: application/json\"\nheader = \"User-Agent: {USER_AGENT}\"\n"
    );
    if let Ok(cookie) = std::env::var("MEMCASTLE_CHATGPT_COOKIE") {
        config.push_str(&format!("header = \"Cookie: {}\"\n", escape(&cookie)?));
    }
    // Secrets go via stdin, never into a command argument or a URI. No redirect may forward them to another origin.
    let response = run_process(
        "curl",
        &[
            // -q must be first: a daemon user's ~/.curlrc must not redirect these secret-bearing requests.
            "-q".into(),
            "--silent".into(),
            "--show-error".into(),
            "--config".into(),
            "-".into(),
            "--max-time".into(),
            "20".into(),
            "--max-redirs".into(),
            "0".into(),
            "--write-out".into(),
            "\nMEMCASTLE_HTTP_STATUS:%{http_code}".into(),
        ],
        Some(config.as_bytes()),
    )
    .map_err(|_| "cannot run curl; check that it is installed and granted".to_string())?;
    if response.status != 0 {
        return Err("ChatGPT request failed; check connectivity, curl and the session (no response is logged)".into());
    }
    let text = String::from_utf8(response.stdout)
        .map_err(|_| "ChatGPT returned non-UTF-8 data".to_string())?;
    let (body, code) = text
        .rsplit_once("\nMEMCASTLE_HTTP_STATUS:")
        .ok_or("ChatGPT response lacks an HTTP status")?;
    match code.trim() {
        "200" => serde_json::from_str(body).map_err(|_| "ChatGPT returned malformed JSON".into()),
        "401" | "403" => {
            Err("ChatGPT refused the session; replace the daemon's session credentials".into())
        }
        "429" => Err("ChatGPT rate-limited this source; retry later".into()),
        "301" | "302" | "307" | "308" => Err(
            "ChatGPT redirected a private API request; refusing to forward session credentials"
                .into(),
        ),
        _ => Err(format!(
            "ChatGPT private API returned HTTP {}; its interface may have changed",
            code.trim()
        )),
    }
}

fn list_status(archived: bool) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    let mut expected_total = None;
    for page in 0..MAX_LIST_PAGES {
        let offset = page * PAGE;
        let response = request(&format!(
            "/backend-api/conversations?offset={offset}&limit={PAGE}&is_archived={archived}"
        ))?;
        let total = response
            .get("total")
            .and_then(Value::as_u64)
            .ok_or("ChatGPT conversation list has no total")?;
        if expected_total.is_some_and(|known| known != total) {
            return Err("ChatGPT conversation list changed during paging; retry the mine".into());
        }
        expected_total = Some(total);
        let items = response
            .get("items")
            .and_then(Value::as_array)
            .ok_or("ChatGPT conversation list has no items")?;
        if items.len() > PAGE {
            return Err("ChatGPT returned an oversized conversation page".into());
        }
        for item in items {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .ok_or("a ChatGPT conversation lacks an id")?;
            if !seen.insert(id.to_string()) {
                return Err(
                    "ChatGPT conversation list shifted during paging; retry the mine".into(),
                );
            }
            ids.push(id.to_string());
        }
        if ids.len() as u64 >= total {
            ids.sort();
            return Ok(ids);
        }
        if items.is_empty() {
            return Err("ChatGPT conversation listing ended before its reported total".into());
        }
    }
    Err(
        "ChatGPT conversation listing exceeds 10000 entries; narrow the source or use an export"
            .into(),
    )
}

pub(super) fn list(selection: Option<&[Project]>) -> Result<Vec<Listing>, String> {
    let mut found: BTreeMap<String, Option<Project>> = BTreeMap::new();
    if selection.is_none() {
        for id in list_status(false)? {
            found.insert(id, None);
        }
        for id in list_status(true)? {
            if found.insert(id, None).is_some() {
                return Err(
                    "a ChatGPT conversation changed archive status during paging; retry the mine"
                        .into(),
                );
            }
        }
    }
    let available;
    let selected = if let Some(selected) = selection {
        selected
    } else {
        available = projects()?;
        &available
    };
    for project in selected {
        for entry in project_conversations(project)? {
            if found
                .get(&entry.id)
                .and_then(Option::as_ref)
                .is_some_and(|known| known.id != project.id)
            {
                return Err(
                    "a ChatGPT conversation appears in multiple projects; retry the mine".into(),
                );
            }
            found.insert(entry.id, Some(project.clone()));
        }
    }
    Ok(found
        .into_iter()
        .map(|(id, project)| Listing { id, project })
        .collect())
}

fn assembled(id: &str, detail: &Value, mut pages: Vec<Vec<Value>>) -> Result<Value, String> {
    pages.reverse();
    let messages: Vec<Value> = pages.into_iter().flatten().collect();
    let mut seen_ids = HashSet::new();
    for message in &messages {
        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .ok_or("a ChatGPT message has no id")?;
        if !seen_ids.insert(message_id) {
            return Err("ChatGPT message pages overlap; refusing a partial transcript".into());
        }
    }
    // Keep every source-specific field from the detail response, except the transient paging state.
    let mut conversation = detail.clone();
    let fields = conversation
        .as_object_mut()
        .ok_or("ChatGPT returned no conversation object")?;
    fields.remove("page_info");
    fields.insert("id".into(), json!(id));
    fields.insert("messages".into(), json!(messages));
    Ok(conversation)
}

pub(super) fn conversation(id: &str) -> Result<Value, String> {
    // An opaque ID is never allowed to rewrite the fixed origin or path.
    if !valid_id(id) {
        return Err("ChatGPT returned an invalid conversation ID".into());
    }
    let mut detail = request(&format!(
        "/backend-api/conversations/{}?num_turns=100&include_has_versions=true",
        encode(id)
    ))?;
    let initial = detail
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("ChatGPT conversation has no messages array")?
        .clone();
    let mut pages = vec![initial];
    let mut seen_cursors = HashSet::new();
    for _ in 0..MAX_MESSAGE_PAGES {
        let info = detail
            .get("page_info")
            .ok_or("ChatGPT conversation has no page_info")?;
        if !info
            .get("has_previous_page")
            .and_then(Value::as_bool)
            .ok_or("ChatGPT conversation has no paging flag")?
        {
            return assembled(id, &detail, pages);
        }
        let before = info
            .get("start_cursor")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("ChatGPT history has no previous-page cursor")?;
        if !seen_cursors.insert(before.to_string()) {
            return Err("ChatGPT message cursor repeated; refusing a partial transcript".into());
        }
        // Keep the initial metadata: older pages may contain only messages and page_info.
        let page = request(&format!(
            "/backend-api/conversations/{}/messages?before={}&num_turns=100&include_has_versions=true",
            encode(id),
            encode(before)
        ))?;
        let messages = page
            .get("messages")
            .and_then(Value::as_array)
            .ok_or("ChatGPT message page has no messages")?;
        if messages.is_empty() {
            return Err("ChatGPT returned an empty page before the end of history".into());
        }
        pages.push(messages.clone());
        let info = page
            .get("page_info")
            .ok_or("ChatGPT message page has no page_info")?;
        if !info
            .get("has_previous_page")
            .and_then(Value::as_bool)
            .ok_or("ChatGPT message page has no paging flag")?
        {
            return assembled(id, &detail, pages);
        }
        detail["page_info"] = info.clone();
    }
    Err("ChatGPT history exceeded the message-page limit; refusing a partial transcript".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mocked<T>(
        handler: impl Fn(&str) -> Result<Value, String> + 'static,
        run: impl FnOnce() -> T,
    ) -> T {
        MOCK_REQUEST.with(|mock| *mock.borrow_mut() = Some(Box::new(handler)));
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                MOCK_REQUEST.with(|mock| *mock.borrow_mut() = None);
            }
        }
        let _reset = Reset;
        run()
    }

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn selectors_and_encoding_reject_ambiguous_or_unsafe_values() {
        let available = [
            project("one", "Shared"),
            project("two", "Shared"),
            project("three", "Other"),
        ];
        assert_eq!(
            select_projects("name:Other,id:one", &available).unwrap(),
            vec![available[0].clone(), available[2].clone()]
        );
        for selection in [
            "",
            "id:",
            "id:../../bad",
            "id:absent",
            "name:",
            "name:Shared",
            "name:absent",
            "name:Other,name:Other",
            "plain",
        ] {
            assert!(
                select_projects(selection, &available).is_err(),
                "{selection}"
            );
        }
        assert!(valid_id("abc_012-xyz"));
        assert!(!valid_id(""));
        assert!(!valid_id(&"a".repeat(129)));
        assert_eq!(encode("a/b ?"), "a%2Fb%20%3F");
        assert_eq!(escape("a\\\"b").unwrap(), "a\\\\\\\"b");
        assert!(escape("line\nbreak").is_err());
        assert!(escape("bad\0header").is_err());
        assert_eq!(next_cursor(&json!({})).unwrap(), None);
        assert_eq!(
            next_cursor(&json!({"cursor":"next"})).unwrap(),
            Some("next")
        );
        for cursor in [
            json!({"cursor":""}),
            json!({"cursor":1}),
            json!({"cursor":"x".repeat(2049)}),
        ] {
            assert!(next_cursor(&cursor).is_err());
        }
    }

    #[test]
    fn project_pages_and_selection_preserve_provenance() {
        mocked(
            |path| {
                if path.contains("sidebar") {
                    Ok(if path.contains("cursor=") {
                        json!({"items":[{"gizmo":{"gizmo":{"id":"two","display":{"name":"Beta"}}}}]})
                    } else {
                        json!({"items":[{"gizmo":{"gizmo":{"id":"one","display":{"name":"Alpha"}}}}],"cursor":"page 2"})
                    })
                } else if path.contains("gizmos/one/") {
                    Ok(json!({"items":[{"id":"chat-1","gizmo_id":"one"}]}))
                } else if path.contains("gizmos/two/") {
                    Ok(json!({"items":[{"id":"chat-2","gizmo_id":"two"}]}))
                } else {
                    Ok(json!({"total":0,"items":[]}))
                }
            },
            || {
                let projects = projects().unwrap();
                assert_eq!(
                    projects,
                    vec![project("one", "Alpha"), project("two", "Beta")]
                );
                let listing = list(None).unwrap();
                assert_eq!(
                    listing
                        .iter()
                        .map(|item| item.id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["chat-1", "chat-2"]
                );
                assert_eq!(listing[0].project.as_ref(), Some(&projects[0]));
                assert_eq!(list(Some(&projects[1..])).unwrap()[0].id, "chat-2");
            },
        );
    }

    #[test]
    fn paging_refuses_inconsistent_project_and_conversation_lists() {
        mocked(
            |path| {
                Ok(
                    json!({"items":[{"gizmo":{"gizmo":{"id":if path.contains("cursor=") { "two" } else { "one" },"display":{"name":"Alpha"}}}}],"cursor":"repeat"}),
                )
            },
            || {
                assert!(projects().unwrap_err().contains("cursor"));
            },
        );
        mocked(
            |_| Ok(json!({"items":[{"id":"chat","gizmo_id":"wrong"}]})),
            || {
                assert!(
                    list(Some(&[project("one", "Alpha")]))
                        .unwrap_err()
                        .contains("another project")
                );
            },
        );
        mocked(
            |_| Ok(json!({"total":2,"items":[{"id":"same"}]})),
            || {
                assert!(list_status(false).unwrap_err().contains("shifted"));
            },
        );
        mocked(
            |path| {
                Ok(if path.contains("is_archived=true") {
                    json!({"total":1,"items":[{"id":"same"}]})
                } else {
                    json!({"total":1,"items":[{"id":"same"}]})
                })
            },
            || {
                assert!(list(None).unwrap_err().contains("archive status"));
            },
        );
        mocked(
            |_| Ok(json!({"items":[]})),
            || assert!(projects().unwrap().is_empty()),
        );
    }

    #[test]
    fn message_pages_are_complete_ordered_and_nonoverlapping() {
        let detail = json!({"title":"Test", "messages":[{"id":"new"}], "page_info":{"has_previous_page":true,"start_cursor":"older"}});
        mocked(
            move |path| {
                Ok(if path.contains("/messages?") {
                    json!({"messages":[{"id":"old"}],"page_info":{"has_previous_page":false}})
                } else {
                    detail.clone()
                })
            },
            || {
                let value = conversation("chat-1").unwrap();
                assert_eq!(value["messages"], json!([{"id":"old"},{"id":"new"}]));
                assert_eq!(value["title"], "Test");
                assert!(value.get("page_info").is_none());
            },
        );
        assert!(conversation("../unsafe").unwrap_err().contains("invalid"));
        mocked(
            |_| {
                Ok(json!({"messages":[],"page_info":{"has_previous_page":true,"start_cursor":"x"}}))
            },
            || {
                assert!(conversation("chat").unwrap_err().contains("empty page"));
            },
        );
        assert!(
            assembled(
                "chat",
                &json!({}),
                vec![vec![json!({"id":"same"}), json!({"id":"same"})]]
            )
            .unwrap_err()
            .contains("overlap")
        );
        assert!(assembled("chat", &json!({}), vec![vec![json!({})]]).is_err());
        assert!(assembled("chat", &json!([]), vec![]).is_err());
    }

    #[test]
    fn rejects_changed_totals_repeated_projects_and_incomplete_history() {
        mocked(
            |path| {
                Ok(if path.contains("offset=0") {
                    json!({"total":21,"items": (0..20).map(|i| json!({"id":format!("id-{i}")})).collect::<Vec<_>>()})
                } else {
                    json!({"total":22,"items":[{"id":"last"}]})
                })
            },
            || {
                assert!(list_status(false).unwrap_err().contains("changed"));
            },
        );
        mocked(
            |path| {
                Ok(if path.contains("cursor=") {
                    json!({"items":[{"id":"chat","gizmo_id":"one"}]})
                } else {
                    json!({"items":[{"id":"chat","gizmo_id":"one"}],"cursor":"next"})
                })
            },
            || {
                assert!(
                    list(Some(&[project("one", "One")]))
                        .unwrap_err()
                        .contains("repeated an ID")
                );
            },
        );
        mocked(
            |path| {
                Ok(if path.contains("cursor=") {
                    json!({"items":[{"gizmo":{"gizmo":{"id":"two","display":{"name":"Other"}}}}],"cursor":"next"})
                } else {
                    json!({"items":[{"gizmo":{"gizmo":{"id":"one","display":{"name":"One"}}}}],"cursor":"next"})
                })
            },
            || {
                assert!(projects().unwrap_err().contains("cursor"));
            },
        );
        mocked(
            |_| {
                Ok(
                    json!({"messages":[{"id":"new"}],"page_info":{"has_previous_page":true,"start_cursor":"same"}}),
                )
            },
            || {
                assert!(
                    conversation("chat")
                        .unwrap_err()
                        .contains("cursor repeated")
                );
            },
        );
        mocked(
            |_| Ok(json!({"messages":[{"id":"new"}]})),
            || {
                assert!(conversation("chat").unwrap_err().contains("page_info"));
            },
        );
        mocked(
            |_| Ok(json!({"messages":[],"page_info":{"has_previous_page":false}})),
            || {
                assert!(
                    conversation("chat").unwrap()["messages"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            },
        );
    }
}
