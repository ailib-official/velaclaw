//! 证据卡片与成文引用检查（VL-RAO-010）。
//!
//! Cards are filled by the tool loop from the call itself. The writeup sample
//! cites those cards. A time range comes from the user text; when the user
//! names none, the newest dated card is the latest status. Covered targets are
//! not executed again.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvidenceCard {
    pub id: String,
    pub tool: String,
    pub command: String,
    pub targets: Vec<String>,
    pub success: bool,
    pub truncated: bool,
    pub start: Option<String>,
    pub end: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskWindow {
    /// User text names no time. Latest-status sentences cite the newest card.
    Latest,
    /// User named one window. Out-of-range cards stay in the pack.
    Specified { start: String, end: Option<String> },
    /// User asked to compare periods. The latest-period default stays off.
    Comparison { dates: Vec<String> },
    /// No dates on the task and no dated cards will be required.
    Undated,
}

pub(crate) fn card_from_call(
    index: usize,
    tool: &str,
    arguments: &str,
    output: &str,
    success: bool,
) -> EvidenceCard {
    let command = command_text(arguments);
    let targets = targets_in(&command);
    let (start, end) = timestamp_span(output);
    EvidenceCard {
        id: format!("c{}", index + 1),
        tool: tool.to_string(),
        command: command.clone(),
        targets,
        success,
        truncated: command_is_truncated(&command),
        start,
        end,
        body: output.to_string(),
    }
}

pub(crate) fn command_text(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(arguments) else {
        return arguments.to_string();
    };
    for key in ["command", "cmd", "path", "file"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            return text.to_string();
        }
    }
    arguments.to_string()
}

pub(crate) fn command_is_truncated(command: &str) -> bool {
    command.split_whitespace().any(|token| {
        let bare = token.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        matches!(bare, "head" | "tail" | "limit")
            || bare.starts_with("--limit")
            || bare.starts_with("--max")
    })
}

pub(crate) fn targets_in(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in command.split_whitespace() {
        let bare = token.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if bare.starts_with('-') || bare.is_empty() {
            continue;
        }
        if (bare.contains('/') || bare.contains('.')) && !out.iter().any(|seen| seen == bare) {
            out.push(bare.to_string());
        }
    }
    out
}

fn timestamp_span(text: &str) -> (Option<String>, Option<String>) {
    let mut found = timestamps_in(text);
    found.sort();
    found.dedup();
    match (found.first(), found.last()) {
        (Some(start), Some(end)) => (Some(start.clone()), Some(end.clone())),
        _ => (None, None),
    }
}

fn timestamps_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 10 <= bytes.len() {
        if let Some(stamp) = parse_stamp_at(text, i) {
            out.push(stamp);
            i += 10;
            continue;
        }
        i += 1;
    }
    out
}

fn parse_stamp_at(text: &str, i: usize) -> Option<String> {
    let rest = text.get(i..)?;
    let date = rest.get(..10)?;
    if !is_date(date) {
        return None;
    }
    let time = rest.get(10..19).filter(|tail| {
        let b = tail.as_bytes();
        (b[0] == b'T' || b[0] == b' ') && is_clock(&tail[1..])
    });
    Some(match time {
        Some(tail) => format!("{date}T{}", &tail[1..]),
        None => format!("{date}T00:00:00"),
    })
}

fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_clock(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 8
        && b[2] == b':'
        && b[5] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 2 || i == 5 || c.is_ascii_digit())
}

pub(crate) fn task_window(user_text: &str, cards: &[EvidenceCard]) -> TaskWindow {
    let dates = timestamps_in(user_text);
    let comparison = is_comparison(user_text);
    if comparison && !dates.is_empty() {
        return TaskWindow::Comparison { dates };
    }
    if let Some(start) = dates.first() {
        let end = if dates.len() >= 2 {
            Some(dates[dates.len() - 1].clone())
        } else {
            None
        };
        return TaskWindow::Specified {
            start: start.clone(),
            end,
        };
    }
    if cards.iter().all(|card| card.end.is_none()) {
        TaskWindow::Undated
    } else {
        TaskWindow::Latest
    }
}

fn is_comparison(user_text: &str) -> bool {
    let lower = user_text.to_ascii_lowercase();
    lower.contains("compare")
        || lower.contains("versus")
        || user_text.contains("对照")
        || user_text.contains("比较")
}

pub(crate) fn newest_card(cards: &[EvidenceCard]) -> Option<&EvidenceCard> {
    cards.iter().max_by(|a, b| a.end.cmp(&b.end))
}

fn day_of(stamp: &str) -> &str {
    stamp.get(..10).unwrap_or(stamp)
}

fn ranges_overlap(card: &EvidenceCard, start: &str, end: Option<&str>) -> bool {
    let Some(card_end) = card.end.as_deref() else {
        return false;
    };
    let card_start = card.start.as_deref().unwrap_or(card_end);
    if day_of(card_end) < day_of(start) {
        return false;
    }
    if let Some(end) = end {
        if day_of(card_start) > day_of(end) {
            return false;
        }
    }
    true
}

fn card_in_window(card: &EvidenceCard, window: &TaskWindow) -> bool {
    match window {
        TaskWindow::Undated | TaskWindow::Latest => true,
        TaskWindow::Specified { start, end } => ranges_overlap(card, start, end.as_deref()),
        TaskWindow::Comparison { dates } => dates
            .iter()
            .any(|date| ranges_overlap(card, date, Some(date))),
    }
}

pub(crate) fn writeup_pack(
    user_text: &str,
    cards: &[EvidenceCard],
    answered_at: &str,
    observation: Option<&str>,
) -> String {
    let window = task_window(user_text, cards);
    let mut out = String::from("[evidence-cards]\n");
    out.push_str("answered_at: ");
    out.push_str(answered_at);
    out.push('\n');
    out.push_str("window: ");
    out.push_str(&window_label(&window));
    out.push('\n');
    for card in cards {
        let range = match (&card.start, &card.end) {
            (Some(start), Some(end)) => format!("{start}..{end}"),
            _ => "none".into(),
        };
        let target = if card.targets.is_empty() {
            "none".to_string()
        } else {
            card.targets.join(",")
        };
        use std::fmt::Write as _;
        let _ = write!(
            out,
            "- {id} tool={tool} target={target} success={success} truncated={truncated} range={range} in_range={in_range}\n{body}\n",
            id = card.id,
            tool = card.tool,
            success = card.success,
            truncated = card.truncated,
            in_range = card_in_window(card, &window),
            body = card.body,
        );
    }
    if let Some(observation) = observation {
        out.push_str("\nObservation: ");
        out.push_str(observation);
        out.push('\n');
    }
    out
}

fn window_label(window: &TaskWindow) -> String {
    match window {
        TaskWindow::Latest => "latest".into(),
        TaskWindow::Undated => "undated".into(),
        TaskWindow::Specified { start, end } => match end {
            Some(end) => format!("{start}..{end}"),
            None => format!("since {start}"),
        },
        TaskWindow::Comparison { dates } => format!("comparison {}", dates.join(",")),
    }
}

pub(crate) fn coverage_line(card: &EvidenceCard) -> String {
    let range = match (&card.start, &card.end) {
        (Some(start), Some(end)) => format!("{start}..{end}"),
        _ => "none".into(),
    };
    let target = if card.targets.is_empty() {
        "none".to_string()
    } else {
        card.targets.join(",")
    };
    format!(
        "[coverage] {id} target={target} range={range} truncated={}",
        card.truncated,
        id = card.id,
    )
}

/// A prior non-truncated card already covers this call's target and range.
pub(crate) fn covered_by<'a>(cards: &'a [EvidenceCard], arguments: &str) -> Option<&'a str> {
    let command = command_text(arguments);
    if command_is_truncated(&command) {
        return None;
    }
    let targets = targets_in(&command);
    if targets.is_empty() {
        return None;
    }
    let (asked_start, _) = timestamp_span(&command);
    cards.iter().rev().find_map(|card| {
        if card.truncated || !card.success || !same_target(card, &targets) {
            return None;
        }
        if let Some(asked) = &asked_start {
            if card.start.as_ref().is_some_and(|start| asked < start) {
                return None;
            }
        }
        Some(card.id.as_str())
    })
}

fn same_target(card: &EvidenceCard, targets: &[String]) -> bool {
    targets
        .iter()
        .any(|target| card.targets.iter().any(|have| have == target))
}

pub(crate) fn citation_issue(
    reply: &str,
    user_text: &str,
    cards: &[EvidenceCard],
) -> Option<String> {
    if cards.is_empty() {
        return None;
    }
    if reply.trim().is_empty() {
        return None;
    }
    let cited = cited_ids(reply);
    if cited.is_empty() {
        return needs_citation(reply).then_some("cite a card id".to_string());
    }
    let window = task_window(user_text, cards);
    if reply_dates_miss_cards(reply, &cited, cards) {
        return Some("cited card does not overlap the sentence time".into());
    }
    if universal_on_truncated(reply, &cited, cards) {
        return Some("truncated card cannot support a universal claim".into());
    }
    if misses_conflicting_card(reply, &cited, cards) {
        return Some("cite every card for this target".into());
    }
    if let Some(issue) = window_issue(reply, &cited, cards, &window) {
        return Some(issue);
    }
    if currency_claim(reply)
        && external_required(user_text)
        && !cites_content_and_external(&cited, cards)
    {
        return Some("cite a content card and an external card".into());
    }
    None
}

fn needs_citation(reply: &str) -> bool {
    reply.contains("最新")
        || reply.contains("全部")
        || reply.contains("完全")
        || reply.contains("成功")
        || reply.contains("失败")
        || reply.contains('%')
        || !timestamps_in(reply).is_empty()
}

fn cited_ids(reply: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = reply;
    while let Some(start) = rest.find("[c") {
        let tail = &rest[start + 1..];
        let Some(end) = tail.find(']') else {
            break;
        };
        let id = &tail[..end];
        if id.starts_with('c') && id[1..].chars().all(|c| c.is_ascii_digit()) {
            let id = id.to_string();
            if !out.contains(&id) {
                out.push(id);
            }
        }
        rest = &tail[end + 1..];
    }
    out
}

fn reply_dates_miss_cards(reply: &str, cited: &[String], cards: &[EvidenceCard]) -> bool {
    let dates = timestamps_in(reply);
    if dates.is_empty() {
        return false;
    }
    cited.iter().any(|id| {
        let Some(card) = cards.iter().find(|card| card.id == *id) else {
            return true;
        };
        if card.end.is_none() {
            return true;
        }
        dates
            .iter()
            .any(|date| !ranges_overlap(card, date, Some(date)))
    })
}

fn universal_on_truncated(reply: &str, cited: &[String], cards: &[EvidenceCard]) -> bool {
    let universal = reply.contains("全部") || reply.contains("完全") || reply.contains('%');
    if !universal {
        return false;
    }
    cited.iter().any(|id| {
        cards
            .iter()
            .find(|card| card.id == *id)
            .is_some_and(|card| card.truncated)
    })
}

fn misses_conflicting_card(reply: &str, cited: &[String], cards: &[EvidenceCard]) -> bool {
    let asserts = reply.contains("成功") || reply.contains("失败");
    if !asserts {
        return false;
    }
    cited.iter().any(|id| {
        let Some(card) = cards.iter().find(|card| card.id == *id) else {
            return false;
        };
        cards.iter().any(|other| {
            other.id != card.id
                && other.success != card.success
                && same_target(other, &card.targets)
                && !cited.iter().any(|seen| seen == &other.id)
        })
    })
}

fn window_issue(
    reply: &str,
    cited: &[String],
    cards: &[EvidenceCard],
    window: &TaskWindow,
) -> Option<String> {
    match window {
        TaskWindow::Undated | TaskWindow::Comparison { .. } => None,
        TaskWindow::Specified { start, end } => {
            let dates = timestamps_in(reply);
            let claims_window = dates.iter().any(|date| {
                date.as_str() >= start.as_str()
                    && end.as_ref().is_none_or(|end| date.as_str() <= end.as_str())
            });
            if !claims_window {
                return None;
            }
            let cited_out = cited.iter().all(|id| {
                cards
                    .iter()
                    .find(|card| card.id == *id)
                    .is_some_and(|card| !card_in_window(card, window))
            });
            if cited_out {
                Some("out-of-range card cannot support this window".into())
            } else {
                None
            }
        }
        TaskWindow::Latest => {
            if !reply.contains("最新") {
                return None;
            }
            let newest = newest_card(cards)?.id.clone();
            if cited.iter().any(|id| id == &newest) {
                None
            } else {
                Some(format!("latest status must cite {newest}"))
            }
        }
    }
}

pub(crate) fn user_task_text(history: &[crate::providers::ChatMessage]) -> String {
    history
        .iter()
        .rev()
        .find(|message| {
            message.role == "user"
                && !message.content.starts_with("[Tool results]")
                && !message.content.starts_with("[coverage]")
                && !message.content.starts_with("[evidence-cards]")
                && !message.content.starts_with("Obligation open:")
        })
        .map(|message| message.content.clone())
        .unwrap_or_default()
}

const NAME_STOP: &[&str] = &[
    "list",
    "files",
    "file",
    "read",
    "update",
    "status",
    "current",
    "align",
    "alignment",
    "git",
    "curl",
    "wget",
    "http",
    "https",
    "www",
    "src",
    "test",
    "log",
    "logs",
    "config",
    "data",
    "repo",
    "code",
    "true",
    "false",
    "none",
    "null",
    "with",
    "from",
    "this",
    "that",
    "what",
    "when",
    "your",
    "the",
    "and",
    "for",
    "all",
    "each",
    "item",
    "items",
    "path",
    "name",
    "names",
    "tmp",
    "var",
    "usr",
    "home",
    "etc",
    "bin",
    "opt",
    "dev",
    "proc",
    "sys",
    "private",
    "users",
];

/// Path-like names in the user text. Ordinary words are not filesystem targets.
pub(crate) fn name_tokens(user_text: &str) -> Vec<String> {
    let stripped = strip_absolute_paths(user_text);
    let mut out = Vec::new();
    let bytes = stripped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if is_name_start(bytes[i]) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_name_cont(bytes[i]) {
                if bytes[i] == b'.'
                    && (i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_alphanumeric())
                {
                    break;
                }
                i += 1;
            }
            let tok = &stripped[start..i];
            if tok.len() >= 3
                && (tok.contains('.') || tok.contains('-') || tok.contains('_'))
                && !NAME_STOP.contains(&tok.to_ascii_lowercase().as_str())
                && !out.iter().any(|seen| seen == tok)
            {
                out.push(tok.to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

fn is_name_cont(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
}

/// Absolute paths written in `text`. Components containing `..` are dropped.
pub(crate) fn absolute_paths_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let boundary = i == 0
            || bytes[i - 1].is_ascii_whitespace()
            || matches!(bytes[i - 1], b'"' | b'\'' | b'=' | b'(' | b'[');
        if bytes[i] == b'/' && boundary {
            let start = i;
            i += 1;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !matches!(bytes[i], b'"' | b'\'' | b',' | b';' | b')' | b']')
            {
                i += 1;
            }
            let mut path = text[start..i].to_string();
            while path.ends_with(['.', ':', ',']) {
                path.pop();
            }
            if path.len() > 1 && !path.split('/').any(|part| part == "..") && !out.contains(&path) {
                out.push(path);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn strip_absolute_paths(text: &str) -> String {
    let mut out = text.to_string();
    for path in absolute_paths_in(text) {
        out = out.replace(&path, " ");
    }
    out
}

pub(crate) fn initial_read_roots(user_text: &str) -> Vec<String> {
    absolute_paths_in(user_text)
}

/// Directories admitted because a successful observation named the user's token.
pub(crate) fn roots_from_observation(
    user_text: &str,
    command: &str,
    output: &str,
    workspace: &std::path::Path,
) -> Vec<String> {
    let tokens = name_tokens(user_text);
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut roots = Vec::new();
    for path in absolute_paths_in(command)
        .into_iter()
        .chain(absolute_paths_in(output))
    {
        if let Some(root) = prefix_through_token(&path, &tokens) {
            push_unique(&mut roots, root);
        }
    }
    let listed_dirs = absolute_paths_in(command);
    for token in &tokens {
        if output_has_bare_name(output, token) {
            for dir in &listed_dirs {
                push_unique(
                    &mut roots,
                    format!("{}/{}", dir.trim_end_matches('/'), token),
                );
            }
        }
        for rel in relative_paths_in(output) {
            if rel.split('/').any(|part| part == token) {
                if let Some(root) =
                    prefix_through_token(&workspace.join(&rel).to_string_lossy(), &tokens)
                {
                    push_unique(&mut roots, root);
                }
            }
        }
    }
    roots
}

fn push_unique(roots: &mut Vec<String>, root: String) {
    if !root.is_empty() && !roots.iter().any(|seen| seen == &root) {
        roots.push(root);
    }
}

fn prefix_through_token(path: &str, tokens: &[String]) -> Option<String> {
    let mut acc = String::new();
    for (index, comp) in path.split('/').filter(|part| !part.is_empty()).enumerate() {
        if path.starts_with('/') && index == 0 {
            acc = format!("/{comp}");
        } else if acc.is_empty() {
            acc = comp.to_string();
        } else {
            acc = format!("{acc}/{comp}");
        }
        if tokens.iter().any(|token| token == comp) {
            return Some(acc);
        }
    }
    None
}

fn output_has_bare_name(output: &str, token: &str) -> bool {
    output.split_whitespace().any(|word| {
        let bare = word.trim_matches(|c| matches!(c, '"' | '\'' | '`' | ',' | ':'));
        bare == token
    })
}

fn relative_paths_in(output: &str) -> Vec<String> {
    output
        .split_whitespace()
        .filter_map(|word| {
            let bare = word.trim_matches(|c| matches!(c, '"' | '\'' | '`' | ','));
            if bare.contains('/') && !bare.starts_with('/') && !bare.contains("..") {
                Some(bare.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn list_only(user_text: &str) -> bool {
    let lower = user_text.to_lowercase();
    let listing = user_text.contains("列出")
        || user_text.contains("列表")
        || user_text.contains("有哪些")
        || lower.contains("list files")
        || lower.contains("what files");
    listing && !external_predicate(user_text) && !content_verb(user_text)
}

fn content_verb(user_text: &str) -> bool {
    user_text.contains("读取")
        || user_text.contains("检查")
        || user_text.contains("查看")
        || has_word(user_text, "read")
        || has_word(user_text, "check")
        || has_word(user_text, "inspect")
}

fn external_predicate(user_text: &str) -> bool {
    user_text.contains("更新")
        || user_text.contains("对齐")
        || user_text.contains("上游")
        || user_text.contains("官方")
        || user_text.contains("现状")
        || has_word(user_text, "update")
        || has_word(user_text, "align")
        || has_word(user_text, "alignment")
        || has_word(user_text, "upstream")
        || has_word(user_text, "official")
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(word))
}

fn has_named_target(user_text: &str) -> bool {
    !name_tokens(user_text).is_empty() || !absolute_paths_in(user_text).is_empty()
}

pub(crate) fn content_required(user_text: &str) -> bool {
    !list_only(user_text) && has_named_target(user_text)
}

pub(crate) fn external_required(user_text: &str) -> bool {
    if list_only(user_text)
        || absolute_paths_in(user_text).len() >= 2
        || !has_named_target(user_text)
    {
        return false;
    }
    external_predicate(user_text)
}

fn each_requested(user_text: &str) -> bool {
    user_text.contains('各')
        || user_text.contains("每一个")
        || user_text.contains("每个")
        || has_word(user_text, "every")
        || has_word(user_text, "each")
}

fn is_content_tool(tool: &str) -> bool {
    matches!(tool, "file_read" | "pdf_read" | "image_info")
}

pub(crate) fn command_is_external(tool: &str, command: &str) -> bool {
    match tool {
        "http_request" | "web_search" | "browser" | "browser_open" => true,
        "shell" => shell_contacts_remote(command),
        _ => false,
    }
}

fn shell_contacts_remote(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|tok| {
        tok.trim_matches(|c| matches!(c, '"' | '\'' | '`'))
            .to_string()
    });
    while let Some(tok) = tokens.next() {
        if is_env_assignment(&tok) {
            continue;
        }
        if tok == "curl" || tok == "wget" || tok == "gh" {
            return true;
        }
        if tok == "git" {
            return tokens.any(|sub| matches!(sub.as_str(), "fetch" | "ls-remote" | "pull"));
        }
        return false;
    }
    false
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

fn file_like(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains('.'))
}

fn under_root(path: &str, roots: &[String]) -> bool {
    let path = std::path::Path::new(path);
    roots
        .iter()
        .any(|root| path.starts_with(std::path::Path::new(root)))
}

fn content_card_matches(card: &EvidenceCard, roots: &[String], tokens: &[String]) -> bool {
    if !card.success || card.truncated || !is_content_tool(&card.tool) {
        return false;
    }
    card.targets.iter().any(|target| {
        under_root(target, roots)
            || tokens
                .iter()
                .any(|token| target.split(['/', '\\']).any(|part| part == token))
    })
}

fn listed_files(cards: &[EvidenceCard], roots: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for card in cards {
        if !card.success || !matches!(card.tool.as_str(), "shell" | "glob_search") {
            continue;
        }
        for path in absolute_paths_in(&card.body) {
            if file_like(&path) && under_root(&path, roots) && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

pub(crate) fn obligation_gap(
    user_text: &str,
    cards: &[EvidenceCard],
    roots: &[String],
) -> Option<String> {
    let tokens = name_tokens(user_text);
    let mut parts = Vec::new();
    if content_required(user_text) {
        if each_requested(user_text) {
            let missing = listed_files(cards, roots)
                .into_iter()
                .filter(|path| {
                    !cards
                        .iter()
                        .any(|card| content_card_matches(card, std::slice::from_ref(path), &tokens))
                })
                .count();
            if missing > 0 {
                parts.push(format!("{missing} listed file(s) were not read"));
            } else if !cards
                .iter()
                .any(|card| content_card_matches(card, roots, &tokens))
            {
                parts.push("the named target has no successful read".into());
            }
        } else if !cards
            .iter()
            .any(|card| content_card_matches(card, roots, &tokens))
        {
            parts.push("the named target has no successful read".into());
        }
    }
    if external_required(user_text)
        && !cards.iter().any(|card| {
            card.success && !card.truncated && command_is_external(&card.tool, &card.command)
        })
    {
        parts.push("no external observation".into());
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("Missing evidence: {}.", parts.join("; ")))
    }
}

fn currency_claim(reply: &str) -> bool {
    reply.contains("已更新")
        || reply.contains("无更新")
        || reply.contains("没有更新")
        || reply.contains("已对齐")
        || reply.contains("未对齐")
        || reply.contains("不一致")
        || has_word(reply, "updated")
        || has_word(reply, "aligned")
}

fn cites_content_and_external(cited: &[String], cards: &[EvidenceCard]) -> bool {
    let chosen: Vec<&EvidenceCard> = cards
        .iter()
        .filter(|card| cited.iter().any(|id| id == &card.id))
        .collect();
    let content = chosen
        .iter()
        .any(|card| is_content_tool(&card.tool) && card.success);
    let external = chosen
        .iter()
        .any(|card| card.success && command_is_external(&card.tool, &card.command));
    content && external
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(
        id: &str,
        target: &str,
        success: bool,
        truncated: bool,
        start: Option<&str>,
        body: &str,
    ) -> EvidenceCard {
        EvidenceCard {
            id: id.into(),
            tool: "shell".into(),
            command: format!("cat {target}"),
            targets: vec![target.into()],
            success,
            truncated,
            start: start.map(str::to_string),
            end: start.map(str::to_string),
            body: body.into(),
        }
    }

    #[test]
    fn dated_card_outside_user_range_cannot_support_in_range_claim() {
        let cards = vec![
            card(
                "c1",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-08-20T00:00:00"),
                "old",
            ),
            card(
                "c2",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-09-29T00:07:00"),
                "new",
            ),
        ];
        let user = "Report since 2026-09-28.";
        let pack = writeup_pack(user, &cards, "2026-09-29T17:00:00Z", None);
        assert!(pack.contains("- c1 "));
        assert!(pack.contains("- c2 "));
        assert!(pack.contains("in_range=false"));
        assert!(pack.contains("in_range=true"));
        let issue = citation_issue("On 2026-09-28 the note failed [c1]", user, &cards);
        assert!(issue.is_some(), "{issue:?}");
        assert!(citation_issue("On 2026-09-29 the note held [c2]", user, &cards).is_none());
    }

    #[test]
    fn unspecified_range_latest_status_cites_newest_card() {
        let cards = vec![
            card(
                "c1",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-08-01T00:00:00"),
                "old",
            ),
            card(
                "c2",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-09-29T00:07:00"),
                "new",
            ),
        ];
        let user = "What is the note?";
        assert_eq!(task_window(user, &cards), TaskWindow::Latest);
        assert!(citation_issue("最新状况是 old [c1]", user, &cards).is_some());
        assert!(citation_issue("最新状况是 new [c2]", user, &cards).is_none());
    }

    #[test]
    fn comparison_request_keeps_both_periods() {
        let cards = vec![
            card(
                "c1",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-08-01T00:00:00"),
                "old",
            ),
            card(
                "c2",
                "/tmp/a.txt",
                true,
                false,
                Some("2026-09-01T00:00:00"),
                "new",
            ),
        ];
        let user = "Compare 2026-08-01 with 2026-09-01.";
        assert!(matches!(
            task_window(user, &cards),
            TaskWindow::Comparison { .. }
        ));
        assert!(citation_issue("August record is old [c1]", user, &cards).is_none());
    }

    #[test]
    fn truncated_card_rejects_universal_claim() {
        let cards = vec![card("c1", "/tmp/a.txt", true, true, None, "one line")];
        let user = "Read the note.";
        assert!(citation_issue("全部完成 [c1]", user, &cards).is_some());
        assert!(citation_issue("样本显示一行 [c1]", user, &cards).is_none());
    }

    #[test]
    fn same_target_conflicting_success_requires_both_citations() {
        let mut fail = card("c1", "/tmp/a.txt", false, false, None, "missing");
        fail.command = "cat /tmp/a.txt".into();
        let mut ok = card("c2", "/tmp/a.txt", true, false, None, "present");
        ok.command = "cat /tmp/a.txt".into();
        let cards = vec![fail, ok];
        let user = "Read the note.";
        assert!(citation_issue("读取成功 [c2]", user, &cards).is_some());
        assert!(citation_issue("读取成功 [c1][c2]", user, &cards).is_none());
    }

    #[test]
    fn undated_cards_skip_time_rule() {
        let cards = vec![card("c1", "/tmp/a.txt", true, false, None, "hello")];
        let user = "Read the note.";
        assert_eq!(task_window(user, &cards), TaskWindow::Undated);
        let pack = writeup_pack(user, &cards, "2026-09-29T17:00:00Z", None);
        assert!(pack.contains("answered_at: 2026-09-29T17:00:00Z"));
        assert!(pack.contains("window: undated"));
        assert!(!pack.contains("window: since 2026-09-29"));
        assert!(citation_issue("the note says hello [c1]", user, &cards).is_none());
    }

    #[test]
    fn covered_target_skips_repeat_call() {
        let cards = vec![card("c1", "/tmp/note.txt", true, false, None, "hello")];
        let again = r#"{"command":"cat /tmp/note.txt"}"#;
        assert_eq!(covered_by(&cards, again), Some("c1"));
    }

    #[test]
    fn truncated_card_allows_wider_call() {
        let mut prior = card("c1", "/tmp/note.txt", true, true, None, "one");
        prior.command = "head -5 /tmp/note.txt".into();
        let wider = r#"{"command":"cat /tmp/note.txt"}"#;
        assert_eq!(covered_by(&[prior], wider), None);
    }

    #[test]
    fn failed_card_does_not_cover() {
        let cards = vec![card(
            "c1",
            "/data/proj-alpha/a.txt",
            false,
            false,
            None,
            "denied",
        )];
        let again = r#"{"path":"/data/proj-alpha/a.txt"}"#;
        assert_eq!(covered_by(&cards, again), None);
    }

    #[test]
    fn user_absolute_path_is_a_read_root() {
        let roots = initial_read_roots("Read /data/proj-alpha/notes.txt please");
        assert_eq!(roots, vec!["/data/proj-alpha/notes.txt".to_string()]);
    }

    #[test]
    fn listed_name_token_admits_that_directory_only() {
        let roots = roots_from_observation(
            "Inspect proj-alpha",
            "ls /data",
            "other\nproj-alpha\n",
            std::path::Path::new("/work"),
        );
        assert_eq!(roots, vec!["/data/proj-alpha".to_string()]);
    }

    #[test]
    fn unrelated_absolute_path_stays_denied() {
        let roots = initial_read_roots("Inspect proj-alpha");
        assert!(roots.is_empty());
        assert!(name_tokens("Inspect proj-alpha") == vec!["proj-alpha".to_string()]);
        assert!(name_tokens("What is the citation?").is_empty());
    }

    #[test]
    fn listing_does_not_satisfy_content() {
        let mut listed = card("c1", "/data/proj-alpha", true, false, None, "notes.txt");
        listed.tool = "shell".into();
        listed.command = "ls /data/proj-alpha".into();
        let user = "Inspect proj-alpha";
        let roots = vec!["/data/proj-alpha".to_string()];
        assert!(content_required(user));
        assert!(obligation_gap(user, &[listed], &roots).is_some());
    }

    #[test]
    fn list_only_request_has_no_content_obligation() {
        let user = "list files in proj-alpha";
        assert!(!content_required(user));
        assert!(!external_required(user));
        assert!(obligation_gap(user, &[], &[]).is_none());
    }

    #[test]
    fn update_with_one_name_requires_external() {
        let user = "Has proj-alpha had an update?";
        assert!(content_required(user));
        assert!(external_required(user));
        let gap = obligation_gap(user, &[], &[]).expect("open");
        assert!(gap.contains("no external observation"));
        assert!(gap.contains("no successful read"));
    }

    #[test]
    fn two_local_paths_skip_external() {
        let user = "Compare /data/proj-alpha/a.txt and /data/proj-beta/b.txt";
        assert!(!external_required(user));
        assert!(content_required(user));
    }

    #[test]
    fn latest_alone_does_not_require_external() {
        let user = "proj-alpha 的最新状况";
        assert!(!external_required(user));
        assert!(content_required(user));
    }

    #[test]
    fn git_status_is_not_external_git_fetch_is() {
        assert!(!command_is_external("shell", "git status"));
        assert!(command_is_external("shell", "git fetch origin"));
        assert!(command_is_external(
            "shell",
            "curl https://example.test/models"
        ));
        assert!(command_is_external(
            "http_request",
            "https://example.test/models"
        ));
        assert!(!command_is_external("shell", "ls /data/proj-alpha"));
    }

    #[test]
    fn currency_sentence_cites_content_and_external() {
        let mut local = card("c1", "/data/proj-alpha/a.txt", true, false, None, "local");
        local.tool = "file_read".into();
        let mut remote = card(
            "c2",
            "https://example.test/models",
            true,
            false,
            None,
            "remote",
        );
        remote.tool = "http_request".into();
        remote.command = "https://example.test/models".into();
        let cards = vec![local, remote];
        let user = "Has proj-alpha had an update?";
        assert!(citation_issue("proj-alpha 无更新 [c1]", user, &cards).is_some());
        assert!(citation_issue("proj-alpha 无更新 [c1][c2]", user, &cards).is_none());
    }
}
