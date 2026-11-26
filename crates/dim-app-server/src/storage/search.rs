use super::{SessionStore, sqlite::error};
use crate::protocol::{SessionSearchHit, SessionSummary};
use dim_agent_runtime::{
    AgentError, Result, TimelineKey, TimelineQuery, TimelineQueryDirection, TimelineQueryOrderBy,
    TimelineQueryScope, TimelineStore,
};
use dim_agent_types::{AgentEvent, ContentBlock, EventKind, ModelOutputOutcome};
use rusqlite::{Connection, params};
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU64,
};
use tokio_util::sync::CancellationToken;

const PAGE: u64 = 256;

pub(super) struct SessionSearchIndex {
    db: Connection,
    scanned: HashMap<String, Scan>,
}

#[derive(Clone, Default)]
struct Scan {
    through: u64,
    inputs: HashMap<String, Document>,
    replies: HashMap<String, ReplyDraft>,
}

#[derive(Clone, Default)]
struct ReplyDraft {
    complete: Vec<Document>,
    current: Option<Document>,
}

#[derive(Clone)]
struct Document {
    sequence: u64,
    time: u64,
    text: String,
}

impl Scan {
    fn apply(&mut self, event: &AgentEvent, complete: &mut Vec<Document>) {
        match &event.kind {
            EventKind::InputMessage(data) => {
                if let Some(user) = &data.user {
                    let text = user
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.trim().is_empty() {
                        self.inputs.insert(
                            event.id.clone(),
                            Document {
                                sequence: event.sequence,
                                time: event.create_at,
                                text,
                            },
                        );
                    }
                }
            }
            EventKind::InputApplied(data) => {
                if let Some(input) = self.inputs.remove(&data.input_id) {
                    complete.push(input);
                }
            }
            EventKind::InputDeleted(data) => {
                self.inputs.remove(&data.input_id);
            }
            EventKind::Text(data) => {
                let reply = self.replies.entry(data.request_id.clone()).or_default();
                if let Some(current) = reply.current.as_mut() {
                    current.text.push_str(&data.delta);
                } else {
                    reply.current = Some(Document {
                        sequence: event.sequence,
                        time: event.create_at,
                        text: data.delta.clone(),
                    });
                }
            }
            EventKind::ToolUse(data) => {
                if let Some(reply) = self.replies.get_mut(&data.request_id)
                    && let Some(current) = reply.current.take()
                {
                    reply.complete.push(current);
                }
            }
            EventKind::ModelOutputEnd(data) => {
                if let Some(reply) = self.replies.remove(&data.request_id)
                    && matches!(data.outcome, ModelOutputOutcome::Completed { .. })
                {
                    complete.extend(
                        reply
                            .complete
                            .into_iter()
                            .chain(reply.current)
                            .filter(|part| !part.text.trim().is_empty()),
                    );
                }
            }
            EventKind::TurnEnd(_) => self.replies.clear(),
            _ => {}
        }
    }
}

impl SessionSearchIndex {
    fn new() -> Result<Self> {
        let db = Connection::open_in_memory().map_err(error)?;
        db.execute_batch(
            "CREATE VIRTUAL TABLE transcript USING fts5(
                body, session_id UNINDEXED, sequence UNINDEXED, time UNINDEXED,
                tokenize = 'trigram'
            )",
        )
        .map_err(error)?;
        Ok(Self {
            db,
            scanned: HashMap::new(),
        })
    }

    fn document_matches(&self, query: &str, limit: usize) -> Result<Vec<SessionSearchHit>> {
        let short = query.chars().count() < 3;
        let (sql, needle) = if short {
            ("SELECT session_id, sequence, body FROM transcript
              WHERE instr(lower(body), lower(?1)) > 0 ORDER BY time DESC, session_id, sequence DESC LIMIT ?2", query.to_owned())
        } else {
            ("SELECT session_id, sequence, body FROM transcript
              WHERE transcript MATCH ?1 ORDER BY rank, time DESC, session_id, sequence DESC LIMIT ?2",
             format!("\"{}\"", query.replace('"', "\"\"")))
        };
        let mut statement = self.db.prepare_cached(sql).map_err(error)?;
        let hits = statement
            .query_map(params![needle, limit as i64], |row| {
                let body: String = row.get(2)?;
                Ok(SessionSearchHit {
                    session_id: row.get(0)?,
                    sequence: row.get::<_, i64>(1)? as u64,
                    snippet: snippet(&body, query),
                })
            })
            .map_err(error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(error)?;
        Ok(hits)
    }
}

fn snippet(text: &str, query: &str) -> String {
    let at = text
        .to_ascii_lowercase()
        .find(&query.to_ascii_lowercase())
        .unwrap_or(0);
    let start = text[..at]
        .char_indices()
        .rev()
        .nth(34)
        .map_or(0, |(index, _)| index);
    let end = text[start..]
        .char_indices()
        .nth(170)
        .map_or(text.len(), |(index, _)| start + index);
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        &text[start..end],
        if end < text.len() { "…" } else { "" }
    )
}

fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(AgentError::Cancelled("Session search cancelled".into()));
    }
    Ok(())
}

impl SessionStore {
    pub(crate) async fn search_transcripts(
        &self,
        sessions: Vec<SessionSummary>,
        query: &str,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<Vec<SessionSearchHit>> {
        let mut slot = tokio::select! {
            slot = self.search.lock() => slot,
            _ = cancel.cancelled() => return Err(AgentError::Cancelled("Session search cancelled".into())),
        };
        if slot.is_none() {
            *slot = Some(SessionSearchIndex::new()?);
        }
        let index = slot.as_mut().expect("search index initialized");
        let visible: HashSet<_> = sessions
            .iter()
            .map(|item| item.session.session_id.as_str())
            .collect();
        let obsolete: Vec<_> = index
            .scanned
            .keys()
            .filter(|id| !visible.contains(id.as_str()))
            .cloned()
            .collect();
        for id in obsolete {
            index
                .db
                .execute("DELETE FROM transcript WHERE session_id = ?1", [&id])
                .map_err(error)?;
            index.scanned.remove(&id);
        }
        let selection = TimelineQuery {
            scope: TimelineQueryScope::Branch,
            include_publishing: false,
            from_event: None,
            from_sequence: None,
            order_by: TimelineQueryOrderBy::Sequence,
            direction: TimelineQueryDirection::Ascending,
            kinds: [
                "input/message",
                "input/applied",
                "input/deleted",
                "step/text",
                "step/tool-use",
                "step/model-output-end",
                "turn/end",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            flags: Vec::new(),
            event_id: None,
            predicate: None,
            limit: NonZeroU64::new(PAGE).expect("positive page size"),
            offset: 0,
        };
        for item in sessions {
            check(&cancel)?;
            let id = item.session.session_id;
            let through = item.session.sequence;
            let key = TimelineKey {
                session_id: id.clone(),
                branch_id: "main".into(),
            };
            if index
                .scanned
                .get(&id)
                .is_some_and(|item| item.through > through)
            {
                index
                    .db
                    .execute("DELETE FROM transcript WHERE session_id = ?1", [&id])
                    .map_err(error)?;
                index.scanned.remove(&id);
            }
            let scan = index.scanned.entry(id.clone()).or_default();
            while scan.through < through {
                let batch = tokio::select! {
                    result = self.timeline.query(&key, &selection, scan.through + 1, through) => result?,
                    _ = cancel.cancelled() => return Err(AgentError::Cancelled("Session search cancelled".into())),
                };
                let mut next = scan.clone();
                let mut documents = Vec::new();
                for event in &batch.events {
                    check(&cancel)?;
                    next.apply(event, &mut documents);
                }
                next.through = if batch.events.len() < PAGE as usize {
                    through
                } else {
                    batch.events.last().expect("full history page").sequence
                };
                let tx = index.db.transaction().map_err(error)?;
                for document in documents {
                    let text = document
                        .text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    tx.execute(
                        "INSERT INTO transcript VALUES (?1, ?2, ?3, ?4)",
                        params![
                            text,
                            id,
                            i64::try_from(document.sequence).map_err(error)?,
                            i64::try_from(document.time).map_err(error)?
                        ],
                    )
                    .map_err(error)?;
                }
                tx.commit().map_err(error)?;
                *scan = next;
            }
        }
        check(&cancel)?;
        index.document_matches(query, limit)
    }
}
