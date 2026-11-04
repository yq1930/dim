//! The `schedule` settings document: the jobs a user writes, and their
//! validation at the document boundary ([design](../../../docs/design/plugins/Scheduler.md),
//! A.1, A.4, §7.1).
use chrono::{DateTime, Local, SecondsFormat, Utc};
use croner::Cron;
use dim_agent_types::{PermissionPolicy, PermissionPreset};
use dim_app_server::settings::{BusinessDocument, SettingsError};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, str::FromStr};

pub const MAX_PROMPT_BYTES: usize = 32 * 1024;
pub const DEFAULT_MAX_RUN_MINUTES: u32 = 60;
pub const DEFAULT_MAX_CONCURRENT_RUNS: u32 = 4;

/// The `schedule` document. `Default` is what Settings uses when no file
/// exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScheduleDocument {
    pub jobs: Vec<Job>,
    /// Retained for document compatibility; it does not control run admission.
    pub max_concurrent_runs: u32,
}

impl Default for ScheduleDocument {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            max_concurrent_runs: DEFAULT_MAX_CONCURRENT_RUNS,
        }
    }
}

impl BusinessDocument for ScheduleDocument {
    const NAMESPACE: &'static str = "schedule";
    const USER_FILE: &'static str = "schedule.json";
    fn validate(&self) -> Result<(), SettingsError> {
        if self.max_concurrent_runs < 1 {
            return Err(SettingsError::Invalid(
                "max_concurrent_runs must be at least 1",
            ));
        }
        let mut ids = HashSet::new();
        for job in &self.jobs {
            if !ids.insert(job.id.as_str()) {
                return Err(SettingsError::Invalid("duplicate job id"));
            }
            job.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Create,
    Reuse,
}

/// Who last authorized a job's save ([design §3.3](../../../docs/design/plugins/Scheduler.md)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "tool")]
    Tool {
        session_id: String,
        branch_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub prompt: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PermissionPreset>,
    pub policy_cap: PermissionPolicy,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_at: Option<String>,
    #[serde(default = "default_max_run_minutes")]
    pub max_run_minutes: u32,
    pub revision: u64,
    pub created_by: Origin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Schedule {
    Cron(CronSchedule),
    Every(EverySchedule),
    At(AtSchedule),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronSchedule {
    pub cron: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EverySchedule {
    pub every_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtSchedule {
    pub at: DateTime<Utc>,
}

/// The time of day a timestamp field holds: UTC with three fractional digits.
pub fn canonical_instant(text: &str) -> Option<DateTime<Utc>> {
    let instant = DateTime::parse_from_rfc3339(text).ok()?.with_timezone(&Utc);
    (instant.to_rfc3339_opts(SecondsFormat::Millis, true) == text).then_some(instant)
}

fn default_max_run_minutes() -> u32 {
    DEFAULT_MAX_RUN_MINUTES
}

pub fn format_instant(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Job {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.name.is_empty() || self.cwd.is_empty() {
            return Err(SettingsError::Invalid("a job needs a name and a cwd"));
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(SettingsError::Invalid("a job prompt is at most 32 KiB"));
        }
        if self.max_run_minutes < 1 {
            return Err(SettingsError::Invalid("max_run_minutes must be at least 1"));
        }
        if self.mode == Mode::Create && self.session_id.is_some() {
            return Err(SettingsError::Invalid("only reuse jobs bind a Session"));
        }
        match (self.enabled, &self.enabled_at) {
            (true, Some(text)) if canonical_instant(text).is_some() => {}
            (false, None) => {}
            _ => return Err(SettingsError::Invalid("enabled_at matches enabled")),
        }
        self.schedule.validate()
    }

    /// The latest window at or before `now`, if any has come due (A.4): after
    /// a gap only this one is considered.
    pub fn latest_window(&self, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let anchor = self.enabled_at.as_deref().and_then(canonical_instant);
        match &self.schedule {
            Schedule::At(at) => Ok((at.at <= now).then_some(at.at)),
            Schedule::Every(every) => {
                let anchor = anchor.ok_or("an every schedule needs enabled_at")?;
                if anchor > now {
                    return Ok(None);
                }
                let k = (now - anchor).num_minutes() / i64::from(every.every_minutes);
                let step = chrono::Duration::minutes(i64::from(every.every_minutes));
                Ok(Some(
                    anchor + step * i32::try_from(k).map_err(|_| "window overflow")?,
                ))
            }
            Schedule::Cron(cron) => {
                let parsed = cron_for(cron)?;
                let mut cursor = now + chrono::Duration::seconds(1);
                for _ in 0..MAX_CRON_SKIPS {
                    let previous = match &cron.tz {
                        Some(_) => parsed
                            .cron
                            .find_previous_occurrence(&cursor.with_timezone(&parsed.zone), false)
                            .map(|previous| previous.with_timezone(&Utc)),
                        None => parsed
                            .cron
                            .find_previous_occurrence(&cursor.with_timezone(&Local), false)
                            .map(|previous| previous.with_timezone(&Utc)),
                    }
                    .map_err(|error| error.to_string())?;
                    if parsed.fires_at(previous, cron.tz.is_some()) {
                        return Ok(Some(previous));
                    }
                    cursor = previous;
                }
                Ok(None)
            }
        }
    }

    /// The first window after `after` (A.4). It is not capped by `now`: a
    /// window already due is still next until the timer runs it.
    pub fn next_window(&self, after: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, String> {
        if !self.enabled {
            return Ok(None);
        }
        self.schedule.next_after(
            after,
            self.enabled_at.as_deref().and_then(canonical_instant),
        )
    }
}

impl Schedule {
    fn validate(&self) -> Result<(), SettingsError> {
        match self {
            Self::Cron(cron) => cron_for(cron)
                .map(|_| ())
                .map_err(|_| SettingsError::Invalid("invalid cron expression or time zone")),
            Self::Every(every) if every.every_minutes >= 1 => Ok(()),
            Self::Every(_) => Err(SettingsError::Invalid("every_minutes must be at least 1")),
            Self::At(_) => Ok(()),
        }
    }

    /// The first window after `after`; an `every` schedule anchors at the
    /// enable time, and an `at` schedule is one instant.
    fn next_after(
        &self,
        after: DateTime<Utc>,
        anchor: Option<DateTime<Utc>>,
    ) -> Result<Option<DateTime<Utc>>, String> {
        match self {
            Self::At(at) => Ok((at.at > after).then_some(at.at)),
            Self::Every(every) => {
                let anchor = anchor.ok_or("an every schedule needs enabled_at")?;
                let step = chrono::Duration::minutes(i64::from(every.every_minutes));
                let first = if anchor > after {
                    anchor
                } else {
                    let elapsed = after - anchor;
                    let k = elapsed.num_minutes() / i64::from(every.every_minutes) + 1;
                    anchor + step * i32::try_from(k).map_err(|_| "window overflow")?
                };
                Ok(Some(first))
            }
            Self::Cron(cron) => {
                let parsed = cron_for(cron).map_err(|error| error.to_string())?;
                let mut cursor = after;
                for _ in 0..MAX_CRON_SKIPS {
                    let next = match &cron.tz {
                        Some(_) => parsed
                            .cron
                            .find_next_occurrence(&cursor.with_timezone(&parsed.zone), false)
                            .map(|next| next.with_timezone(&Utc)),
                        None => parsed
                            .cron
                            .find_next_occurrence(&cursor.with_timezone(&Local), false)
                            .map(|next| next.with_timezone(&Utc)),
                    }
                    .map_err(|error| error.to_string())?;
                    if parsed.fires_at(next, cron.tz.is_some()) {
                        return Ok(Some(next));
                    }
                    cursor = next;
                }
                Ok(None)
            }
        }
    }
}

/// How many DST-shifted or repeated occurrences a cron search passes over
/// before giving up; each skip is at least one minute.
const MAX_CRON_SKIPS: usize = 8;

impl ParsedCron {
    /// Whether `instant` is a real window (A.4): its wall time in the zone
    /// matches the pattern (a time inside a DST gap does not exist, so the
    /// occurrence moved out of it is skipped), and it is the first instant
    /// with that wall time (a repeated fall-back time fires once).
    fn fires_at(&self, instant: DateTime<Utc>, zoned: bool) -> bool {
        if zoned {
            first_of_wall_time(&self.cron, instant, &self.zone)
        } else {
            first_of_wall_time(&self.cron, instant, &Local)
        }
    }
}

struct ParsedCron {
    cron: Cron,
    zone: chrono_tz::Tz,
}

/// Whether the wall time of `instant` in `zone` matches `cron` and no earlier
/// instant has that wall time: the zone resolves it to `instant` or, when the
/// time repeats at a fall-back, to its earlier occurrence.
fn first_of_wall_time<Z: chrono::TimeZone>(cron: &Cron, instant: DateTime<Utc>, zone: &Z) -> bool
where
    DateTime<Z>: croner::CronDateTime,
{
    let local = instant.with_timezone(zone);
    cron.is_time_matching(&local).unwrap_or(false)
        && zone
            .from_local_datetime(&local.naive_local())
            .earliest()
            .is_some_and(|first| first.with_timezone(&Utc) == instant)
}

/// Five-field cron with an optional IANA time zone (A.4).
fn cron_for(cron: &CronSchedule) -> Result<ParsedCron, String> {
    if cron.cron.split_whitespace().count() != 5 {
        return Err("cron needs five fields".into());
    }
    let zone = match &cron.tz {
        Some(name) => name
            .parse::<chrono_tz::Tz>()
            .map_err(|_| format!("unknown time zone {name}"))?,
        None => chrono_tz::UTC,
    };
    let parsed = Cron::from_str(&cron.cron).map_err(|error| error.to_string())?;
    Ok(ParsedCron { cron: parsed, zone })
}
