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

fn targets_in(command: &str) -> Vec<String> {
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
        if card.truncated || !same_target(card, &targets) {
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
        })
        .map(|message| message.content.clone())
        .unwrap_or_default()
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
}
