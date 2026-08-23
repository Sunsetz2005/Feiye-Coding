//! App-resident automation scheduler.
//!
//! Rust owns due-time polling, atomic claims, leases, a bounded run ledger, and
//! one catch-up attempt. Claimed prompts still execute through the existing
//! Tauri Host → ACP → Runtime path in the WebView. System-level scheduling while
//! the app is terminated is intentionally out of scope.

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::store::Automation;

const CLAIM_LEASE_MINUTES: i64 = 10;
const MAX_LEDGER_ROWS: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatusV1 {
    Claimed,
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunLedgerEntryV1 {
    pub version: u8,
    pub claim_id: String,
    pub automation_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub scheduled_for: DateTime<Utc>,
    pub claimed_at: DateTime<Utc>,
    pub lease_until: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub catch_up: bool,
    pub status: AutomationRunStatusV1,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationClaimV1 {
    pub version: u8,
    pub claim_id: String,
    pub scheduled_for: DateTime<Utc>,
    pub catch_up: bool,
    pub session_id: Option<String>,
    pub automation: Automation,
}

fn parse_hhmm(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

fn day_matches(automation: &Automation, weekday: u32) -> bool {
    match automation.frequency.trim().to_ascii_lowercase().as_str() {
        "weekdays" => (1..=5).contains(&weekday),
        "weekly" => {
            let configured = if automation.weekdays.is_empty() {
                vec![automation
                    .created_at
                    .with_timezone(&Local)
                    .weekday()
                    .num_days_from_sunday() as u8]
            } else {
                automation.weekdays.clone()
            };
            configured.contains(&(weekday as u8))
        }
        _ => true,
    }
}

fn local_slot(automation: &Automation, local_day: chrono::NaiveDate) -> Option<DateTime<Utc>> {
    let (hour, minute) = parse_hhmm(&automation.time)?;
    if !day_matches(automation, local_day.weekday().num_days_from_sunday()) {
        return None;
    }
    Local
        .from_local_datetime(&local_day.and_hms_opt(hour, minute, 0)?)
        .earliest()
        .map(|value| value.with_timezone(&Utc))
}

fn latest_due_slot(automation: &Automation, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Some(next) = automation.next_run_at {
        return (next <= now).then_some(next);
    }
    let local_now = now.with_timezone(&Local);
    for offset in 0..14 {
        let day = local_now.date_naive() - ChronoDuration::days(offset);
        let Some(slot) = local_slot(automation, day) else {
            continue;
        };
        if slot <= now && automation.last_run_at.is_none_or(|last| last < slot) {
            return Some(slot);
        }
    }
    None
}

fn next_slot_after(automation: &Automation, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if automation.frequency.eq_ignore_ascii_case("once") {
        return None;
    }
    let local_after = after.with_timezone(&Local);
    for offset in 0..15 {
        let day = local_after.date_naive() + ChronoDuration::days(offset);
        let Some(slot) = local_slot(automation, day) else {
            continue;
        };
        if slot > after {
            return Some(slot);
        }
    }
    None
}

fn load_ledger() -> Vec<AutomationRunLedgerEntryV1> {
    std::fs::read_to_string(crate::paths::automation_runs_file())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn expire_claims(now: DateTime<Utc>) -> Result<(), String> {
    let expired = crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            let mut automation_ids = Vec::new();
            for entry in ledger.iter_mut() {
                if entry.status == AutomationRunStatusV1::Claimed && entry.lease_until <= now {
                    entry.status = AutomationRunStatusV1::Interrupted;
                    entry.completed_at = Some(now);
                    entry.error = Some("claim lease expired before WebView completion".into());
                    automation_ids.push(entry.automation_id.clone());
                }
            }
            Ok(automation_ids)
        },
    )?;
    for automation_id in expired {
        if let Some(automation) = crate::store::load_automations()
            .into_iter()
            .find(|item| item.id == automation_id)
        {
            let next = next_slot_after(&automation, now + ChronoDuration::minutes(1));
            let _ = crate::store::scheduler_advance_automation(
                &automation.id,
                now,
                next,
                automation.frequency.eq_ignore_ascii_case("once"),
            );
        }
    }
    Ok(())
}

fn active_claim() -> Result<Option<AutomationClaimV1>, String> {
    let Some(entry) = load_ledger()
        .into_iter()
        .find(|entry| entry.status == AutomationRunStatusV1::Claimed)
    else {
        return Ok(None);
    };
    let automation = crate::store::load_automations()
        .into_iter()
        .find(|automation| automation.id == entry.automation_id);
    let Some(automation) = automation else {
        let now = Utc::now();
        crate::store_lock::update_json_locked(
            &crate::paths::automation_runs_file(),
            Vec::<AutomationRunLedgerEntryV1>::new,
            |ledger| {
                if let Some(stored) = ledger
                    .iter_mut()
                    .find(|stored| stored.claim_id == entry.claim_id)
                {
                    stored.status = AutomationRunStatusV1::Interrupted;
                    stored.completed_at = Some(now);
                    stored.error = Some("automation was deleted while claimed".into());
                }
                Ok(())
            },
        )?;
        return Ok(None);
    };
    Ok(Some(AutomationClaimV1 {
        version: 1,
        claim_id: entry.claim_id,
        scheduled_for: entry.scheduled_for,
        catch_up: entry.catch_up,
        session_id: entry.session_id,
        automation,
    }))
}

fn claim_due_once(now: DateTime<Utc>) -> Result<Option<AutomationClaimV1>, String> {
    expire_claims(now)?;
    if let Some(claim) = active_claim()? {
        return Ok(Some(claim));
    }
    let Some((automation, scheduled_for)) = crate::store::load_automations()
        .into_iter()
        .filter(|automation| automation.enabled)
        .filter_map(|automation| {
            latest_due_slot(&automation, now).map(|scheduled| (automation, scheduled))
        })
        .min_by_key(|(_, scheduled)| *scheduled)
    else {
        return Ok(None);
    };

    if load_ledger()
        .iter()
        .any(|entry| entry.automation_id == automation.id && entry.scheduled_for == scheduled_for)
    {
        // A catch-up occurrence runs at most once. Advance instead of looping.
        let next = next_slot_after(&automation, now + ChronoDuration::minutes(1));
        let _ = crate::store::scheduler_advance_automation(
            &automation.id,
            now,
            next,
            automation.frequency.eq_ignore_ascii_case("once"),
        );
        return Ok(None);
    }

    let claim_id = Uuid::new_v4().to_string();
    let catch_up = now.signed_duration_since(scheduled_for) > ChronoDuration::seconds(90);
    let entry = AutomationRunLedgerEntryV1 {
        version: 1,
        claim_id: claim_id.clone(),
        automation_id: automation.id.clone(),
        session_id: None,
        scheduled_for,
        claimed_at: now,
        lease_until: now + ChronoDuration::minutes(CLAIM_LEASE_MINUTES),
        completed_at: None,
        catch_up,
        status: AutomationRunStatusV1::Claimed,
        error: None,
    };
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            if ledger.iter().any(|known| {
                known.automation_id == entry.automation_id
                    && known.scheduled_for == entry.scheduled_for
            }) {
                return Err("automation occurrence already claimed".into());
            }
            ledger.push(entry.clone());
            if ledger.len() > MAX_LEDGER_ROWS {
                let drain = ledger.len() - MAX_LEDGER_ROWS;
                ledger.drain(0..drain);
            }
            Ok(())
        },
    )?;
    Ok(Some(AutomationClaimV1 {
        version: 1,
        claim_id,
        scheduled_for,
        catch_up,
        session_id: None,
        automation,
    }))
}

pub fn bind_session(claim_id: &str, session_id: &str) -> Result<(), String> {
    let session_id = session_id.trim();
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("invalid automation session id".into());
    }
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            let entry = ledger
                .iter_mut()
                .find(|entry| entry.claim_id == claim_id)
                .ok_or_else(|| "automation claim not found".to_string())?;
            if entry.status != AutomationRunStatusV1::Claimed {
                return Err("automation claim already completed".into());
            }
            if entry
                .session_id
                .as_deref()
                .is_some_and(|known| known != session_id)
            {
                return Err("automation claim is already bound to another session".into());
            }
            entry.session_id = Some(session_id.to_string());
            Ok(())
        },
    )
}

pub fn complete_for_session(
    session_id: &str,
    success: bool,
    error: Option<&str>,
) -> Result<bool, String> {
    let claim_id = load_ledger()
        .into_iter()
        .find(|entry| {
            entry.status == AutomationRunStatusV1::Claimed
                && entry.session_id.as_deref() == Some(session_id)
        })
        .map(|entry| entry.claim_id);
    let Some(claim_id) = claim_id else {
        return Ok(false);
    };
    complete(&claim_id, success, error)?;
    Ok(true)
}

pub fn complete(claim_id: &str, success: bool, error: Option<&str>) -> Result<(), String> {
    let now = Utc::now();
    let automation_id = load_ledger()
        .into_iter()
        .find(|entry| entry.claim_id == claim_id)
        .map(|entry| entry.automation_id)
        .ok_or_else(|| "automation claim not found".to_string())?;
    let automation = crate::store::load_automations()
        .into_iter()
        .find(|automation| automation.id == automation_id)
        .ok_or_else(|| "automation not found".to_string())?;
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            let entry = ledger
                .iter_mut()
                .find(|entry| entry.claim_id == claim_id)
                .ok_or_else(|| "automation claim not found".to_string())?;
            if entry.status != AutomationRunStatusV1::Claimed {
                return Err("automation claim already completed".into());
            }
            entry.status = if success {
                AutomationRunStatusV1::Succeeded
            } else {
                AutomationRunStatusV1::Failed
            };
            entry.completed_at = Some(now);
            entry.error = error
                .map(|value| value.chars().take(500).collect::<String>())
                .filter(|value| !value.is_empty());
            Ok(())
        },
    )?;

    // The ledger is the idempotency boundary. If advancing the automation file
    // fails, the next scheduler tick observes the completed occurrence and
    // retries only the schedule advance instead of executing the prompt again.
    let next = next_slot_after(&automation, now + ChronoDuration::minutes(1));
    crate::store::scheduler_advance_automation(
        &automation.id,
        now,
        next,
        automation.frequency.eq_ignore_ascii_case("once"),
    )?;
    Ok(())
}

fn tick(app: &AppHandle) {
    match claim_due_once(Utc::now()) {
        Ok(Some(claim)) => {
            let _ = app.emit("automation://claim_v1", claim);
        }
        Ok(None) => {}
        Err(error) => tracing::warn!("automation scheduler tick failed: {error}"),
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let app = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || tick(&app)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn automation(frequency: &str, time: &str, weekdays: Vec<u8>) -> Automation {
        let now = Utc::now();
        Automation {
            id: "a".into(),
            title: "test".into(),
            prompt: "run".into(),
            enabled: true,
            project_id: None,
            model_id: None,
            effort: None,
            frequency: frequency.into(),
            time: time.into(),
            weekdays,
            notify: "none".into(),
            created_at: now,
            updated_at: now,
            last_run_at: None,
            next_run_at: None,
        }
    }

    #[test]
    fn invalid_time_never_schedules() {
        assert!(latest_due_slot(&automation("daily", "25:99", vec![]), Utc::now()).is_none());
    }

    #[test]
    fn once_has_no_followup_slot() {
        assert!(next_slot_after(&automation("once", "09:00", vec![]), Utc::now()).is_none());
    }

    #[test]
    fn weekly_without_explicit_days_uses_creation_weekday() {
        let automation = automation("weekly", "09:00", vec![]);
        let created_weekday = automation
            .created_at
            .with_timezone(&Local)
            .weekday()
            .num_days_from_sunday();
        assert!(day_matches(&automation, created_weekday));
        assert!(!day_matches(&automation, (created_weekday + 1) % 7));
    }

    #[test]
    fn time_parser_accepts_only_complete_clock_values() {
        assert_eq!(parse_hhmm(" 00:00 "), Some((0, 0)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("noon"), None);
        assert_eq!(parse_hhmm("1:2:3"), None);
    }

    #[test]
    fn frequency_day_matching_covers_daily_weekdays_and_configured_weekly() {
        let daily = automation("daily", "09:00", vec![]);
        assert!((0..=6).all(|weekday| day_matches(&daily, weekday)));

        let weekdays = automation("weekdays", "09:00", vec![]);
        assert!(!day_matches(&weekdays, 0));
        assert!(day_matches(&weekdays, 1));
        assert!(day_matches(&weekdays, 5));
        assert!(!day_matches(&weekdays, 6));

        let weekly = automation("weekly", "09:00", vec![0, 3]);
        assert!(day_matches(&weekly, 0));
        assert!(day_matches(&weekly, 3));
        assert!(!day_matches(&weekly, 4));
    }

    #[test]
    fn explicit_next_run_is_due_only_after_its_timestamp() {
        let now = Utc::now();
        let mut scheduled = automation("daily", "09:00", vec![]);
        scheduled.next_run_at = Some(now + ChronoDuration::minutes(1));
        assert!(latest_due_slot(&scheduled, now).is_none());

        let due = now - ChronoDuration::minutes(1);
        scheduled.next_run_at = Some(due);
        assert_eq!(latest_due_slot(&scheduled, now), Some(due));
    }

    #[test]
    fn local_slots_respect_last_run_and_advance_to_the_next_day() {
        let mut daily = automation("daily", "00:00", vec![]);
        let local_today = Local::now().date_naive();
        let today = local_slot(&daily, local_today).expect("midnight should resolve locally");
        let now = today + ChronoDuration::minutes(1);

        assert_eq!(latest_due_slot(&daily, now), Some(today));
        daily.last_run_at = Some(today);
        assert!(latest_due_slot(&daily, now).is_none());

        daily.last_run_at = None;
        let next = next_slot_after(&daily, today).expect("daily schedule should continue");
        assert!(next > today);
        assert_eq!(
            next.with_timezone(&Local).date_naive(),
            local_today.succ_opt().unwrap()
        );

        let weekday = local_today.weekday().num_days_from_sunday();
        let weekly_other_day = automation("weekly", "00:00", vec![((weekday + 1) % 7) as u8]);
        assert!(local_slot(&weekly_other_day, local_today).is_none());
    }
}
