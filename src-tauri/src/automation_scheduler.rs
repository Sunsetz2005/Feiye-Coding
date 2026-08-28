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
const MISSED_RUN_AFTER_SECONDS: i64 = 90;
const LEASE_EXPIRED_ERROR: &str = "claim lease expired before scheduler completion";
const PROCESS_EXITED_ERROR: &str = "runtime process exited before scheduler completion";
const MAX_PROCESS_ID_CHARS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatusV1 {
    Claimed,
    Succeeded,
    Failed,
    Interrupted,
    Skipped,
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
    #[serde(default)]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_runtime_session_sequence: Option<u64>,
    #[serde(default)]
    pub runtime_process_identity: Option<AutomationRuntimeProcessIdentityV1>,
    #[serde(default)]
    pub replaces_claim_id: Option<String>,
    #[serde(default)]
    pub replacement_claim_id: Option<String>,
    #[serde(default)]
    pub termination_proof: Option<AutomationTerminationProofV1>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTerminationProofKindV1 {
    ProcessExit,
    CancelAck,
}

/// Host-observed evidence that the Runtime process which owned a claim can no
/// longer produce side effects for it. Missing heartbeat is not this proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationTerminationProofV1 {
    pub version: u8,
    pub proof_id: String,
    pub kind: AutomationTerminationProofKindV1,
    pub claim_id: String,
    pub session_id: String,
    pub process_id: String,
    pub runtime_session_sequence: u64,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationRuntimeProcessIdentityV1 {
    pub version: u8,
    pub session_id: String,
    pub process_id: String,
    pub first_runtime_session_sequence: u64,
    pub bound_at: DateTime<Utc>,
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

/// Host-internal progress evidence. Callers must construct this only from a
/// RuntimeEventEnvelopeV1 for the claim's bound Runtime session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationRuntimeHeartbeatV1 {
    pub version: u8,
    pub claim_id: String,
    pub session_id: String,
    pub process_id: String,
    pub runtime_session_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRuntimeHeartbeatAckV1 {
    pub version: u8,
    pub claim_id: String,
    pub session_id: String,
    pub runtime_session_sequence: u64,
    pub recorded_at: DateTime<Utc>,
    pub lease_until: DateTime<Utc>,
}

/// Policy for an occurrence discovered after its scheduled wall-clock slot.
/// App-resident scheduling preserves the historical run-once behavior.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicyV2 {
    Skip,
    #[default]
    RunOnce,
}

impl MissedRunPolicyV2 {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "skip" => Self::Skip,
            _ => Self::RunOnce,
        }
    }
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

fn is_lease_interruption(entry: &AutomationRunLedgerEntryV1) -> bool {
    entry.status == AutomationRunStatusV1::Interrupted
        && matches!(
            entry.error.as_deref(),
            Some(
                LEASE_EXPIRED_ERROR
                    | PROCESS_EXITED_ERROR
                    | "claim lease expired before WebView completion"
            )
        )
}

fn valid_process_id(process_id: &str) -> bool {
    !process_id.is_empty()
        && process_id.len() <= MAX_PROCESS_ID_CHARS
        && process_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn retryable_interrupted_original<'a>(
    ledger: &'a [AutomationRunLedgerEntryV1],
    automation_id: &str,
    scheduled_for: DateTime<Utc>,
) -> Option<&'a AutomationRunLedgerEntryV1> {
    ledger.iter().find(|entry| {
        entry.automation_id == automation_id
            && entry.scheduled_for == scheduled_for
            && is_lease_interruption(entry)
            && entry.replacement_claim_id.is_none()
            && termination_proof_authorizes_replacement(entry)
    })
}

fn termination_proof_authorizes_replacement(entry: &AutomationRunLedgerEntryV1) -> bool {
    let Some(identity) = entry.runtime_process_identity.as_ref() else {
        return false;
    };
    let Some(proof) = entry.termination_proof.as_ref() else {
        return false;
    };
    proof.version == 1
        && proof.claim_id == entry.claim_id
        && proof.session_id == identity.session_id
        && proof.process_id == identity.process_id
        && proof.runtime_session_sequence >= identity.first_runtime_session_sequence
        && matches!(
            proof.kind,
            AutomationTerminationProofKindV1::ProcessExit
                | AutomationTerminationProofKindV1::CancelAck
        )
}

fn trim_ledger(ledger: &mut Vec<AutomationRunLedgerEntryV1>) {
    if ledger.len() <= MAX_LEDGER_ROWS {
        return;
    }
    let excess = ledger.len() - MAX_LEDGER_ROWS;
    let mut removable = ledger
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.status != AutomationRunStatusV1::Claimed)
        .map(|(index, entry)| (index, entry.claimed_at, entry.claim_id.clone()))
        .collect::<Vec<_>>();
    removable.sort_by(|left, right| (left.1, &left.2).cmp(&(right.1, &right.2)));
    let mut remove_indices = removable
        .into_iter()
        .take(excess)
        .map(|(index, _, _)| index)
        .collect::<Vec<_>>();
    remove_indices.sort_unstable_by(|left, right| right.cmp(left));
    for index in remove_indices {
        ledger.remove(index);
    }
}

fn expire_claims_in_ledger(ledger: &mut Vec<AutomationRunLedgerEntryV1>, now: DateTime<Utc>) {
    for entry in ledger.iter_mut() {
        if entry.status == AutomationRunStatusV1::Claimed && entry.lease_until <= now {
            entry.status = AutomationRunStatusV1::Interrupted;
            entry.completed_at = Some(now);
            entry.error = Some(LEASE_EXPIRED_ERROR.into());
        }
    }
    trim_ledger(ledger);
}

fn expire_claims(now: DateTime<Utc>) -> Result<(), String> {
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            expire_claims_in_ledger(ledger, now);
            Ok(())
        },
    )
}

fn claim_from_entry(
    entry: AutomationRunLedgerEntryV1,
    automations: &[Automation],
) -> Result<AutomationClaimV1, String> {
    let automation = automations
        .iter()
        .find(|automation| automation.id == entry.automation_id)
        .cloned()
        .ok_or_else(|| "automation not found for active claim".to_string())?;
    Ok(AutomationClaimV1 {
        version: 1,
        claim_id: entry.claim_id,
        scheduled_for: entry.scheduled_for,
        catch_up: entry.catch_up,
        session_id: entry.session_id,
        automation,
    })
}

fn new_claim_entry(
    automation_id: &str,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
    catch_up: bool,
) -> AutomationRunLedgerEntryV1 {
    AutomationRunLedgerEntryV1 {
        version: 1,
        claim_id: Uuid::new_v4().to_string(),
        automation_id: automation_id.to_string(),
        session_id: None,
        scheduled_for,
        claimed_at: now,
        lease_until: now + ChronoDuration::minutes(CLAIM_LEASE_MINUTES),
        completed_at: None,
        catch_up,
        status: AutomationRunStatusV1::Claimed,
        error: None,
        last_heartbeat_at: None,
        last_runtime_session_sequence: None,
        runtime_process_identity: None,
        replaces_claim_id: None,
        replacement_claim_id: None,
        termination_proof: None,
    }
}

fn replace_interrupted_claim_in_ledger(
    ledger: &mut Vec<AutomationRunLedgerEntryV1>,
    original_claim_id: &str,
    now: DateTime<Utc>,
) -> Result<AutomationRunLedgerEntryV1, String> {
    let original_index = ledger
        .iter()
        .position(|entry| entry.claim_id == original_claim_id)
        .ok_or_else(|| "automation claim not found".to_string())?;
    let original = ledger[original_index].clone();
    if !is_lease_interruption(&original) {
        return Err("automation claim is not a retryable interruption".into());
    }
    if original.replacement_claim_id.is_some() {
        return Err("automation claim already has a replacement".into());
    }
    if !termination_proof_authorizes_replacement(&original) {
        return Err("automation termination proof does not authorize replacement".into());
    }
    if ledger
        .iter()
        .any(|entry| entry.status == AutomationRunStatusV1::Claimed)
    {
        return Err("another automation claim is still active".into());
    }

    let mut replacement = new_claim_entry(
        &original.automation_id,
        original.scheduled_for,
        now,
        true,
    );
    replacement.replaces_claim_id = Some(original.claim_id.clone());
    ledger[original_index].replacement_claim_id = Some(replacement.claim_id.clone());
    ledger.push(replacement.clone());
    trim_ledger(ledger);
    Ok(replacement)
}

enum LedgerClaimDecision {
    Claimed(AutomationRunLedgerEntryV1),
    Existing(AutomationRunLedgerEntryV1),
    Suppressed,
}

fn claim_occurrence_in_ledger(
    ledger: &mut Vec<AutomationRunLedgerEntryV1>,
    automation_id: &str,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
    policy: MissedRunPolicyV2,
    recovery: bool,
) -> LedgerClaimDecision {
    if let Some(active) = ledger
        .iter()
        .find(|entry| entry.status == AutomationRunStatusV1::Claimed)
        .cloned()
    {
        return LedgerClaimDecision::Existing(active);
    }

    if recovery {
        if policy == MissedRunPolicyV2::Skip {
            trim_ledger(ledger);
            return LedgerClaimDecision::Suppressed;
        }
        if let Some(original_id) = retryable_interrupted_original(ledger, automation_id, scheduled_for)
            .map(|entry| entry.claim_id.clone())
        {
            match replace_interrupted_claim_in_ledger(ledger, &original_id, now) {
                Ok(replacement) => return LedgerClaimDecision::Claimed(replacement),
                Err(_) => {
                    trim_ledger(ledger);
                    return LedgerClaimDecision::Suppressed;
                }
            }
        }
        // A fixed lease cannot prove that the Runtime stopped executing.
        trim_ledger(ledger);
        return LedgerClaimDecision::Suppressed;
    }

    let same_occurrence = ledger
        .iter()
        .any(|entry| entry.automation_id == automation_id && entry.scheduled_for == scheduled_for);
    if same_occurrence {
        return LedgerClaimDecision::Suppressed;
    }

    let missed = now.signed_duration_since(scheduled_for)
        > ChronoDuration::seconds(MISSED_RUN_AFTER_SECONDS);
    if missed && policy == MissedRunPolicyV2::Skip {
        ledger.push(AutomationRunLedgerEntryV1 {
            version: 1,
            claim_id: Uuid::new_v4().to_string(),
            automation_id: automation_id.to_string(),
            session_id: None,
            scheduled_for,
            claimed_at: now,
            lease_until: now,
            completed_at: Some(now),
            catch_up: true,
            status: AutomationRunStatusV1::Skipped,
            error: Some("missed run skipped by policy".into()),
            last_heartbeat_at: None,
            last_runtime_session_sequence: None,
            runtime_process_identity: None,
            replaces_claim_id: None,
            replacement_claim_id: None,
            termination_proof: None,
        });
        trim_ledger(ledger);
        return LedgerClaimDecision::Suppressed;
    }

    let entry = new_claim_entry(automation_id, scheduled_for, now, missed);
    ledger.push(entry.clone());
    trim_ledger(ledger);
    LedgerClaimDecision::Claimed(entry)
}

fn advance_after_occurrence(automation: &Automation, now: DateTime<Utc>) -> Result<(), String> {
    let next = next_slot_after(automation, now + ChronoDuration::minutes(1));
    crate::store::scheduler_advance_automation(
        &automation.id,
        now,
        next,
        automation.frequency.eq_ignore_ascii_case("once"),
    )
    .map(|_| ())
}

fn resolve_claim_decision(
    decision: LedgerClaimDecision,
    automations: &[Automation],
    selected: &Automation,
    now: DateTime<Utc>,
) -> Result<Option<AutomationClaimV1>, String> {
    match decision {
        LedgerClaimDecision::Claimed(entry) | LedgerClaimDecision::Existing(entry) => {
            Ok(Some(claim_from_entry(entry, automations)?))
        }
        LedgerClaimDecision::Suppressed => {
            advance_after_occurrence(selected, now)?;
            Ok(None)
        }
    }
}

fn active_claim(now: DateTime<Utc>) -> Result<Option<AutomationClaimV1>, String> {
    let Some(entry) = load_ledger()
        .into_iter()
        .find(|entry| entry.status == AutomationRunStatusV1::Claimed)
    else {
        return Ok(None);
    };
    let automations = crate::store::load_automations();
    if automations
        .iter()
        .any(|automation| automation.id == entry.automation_id)
    {
        return claim_from_entry(entry, &automations).map(Some);
    }
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            if let Some(stored) = ledger
                .iter_mut()
                .find(|stored| stored.claim_id == entry.claim_id)
            {
                if stored.status == AutomationRunStatusV1::Claimed {
                    stored.status = AutomationRunStatusV1::Interrupted;
                    stored.completed_at = Some(now);
                    stored.error = Some("automation was deleted while claimed".into());
                }
            }
            trim_ledger(ledger);
            Ok(())
        },
    )?;
    Ok(None)
}

fn claim_due_once(now: DateTime<Utc>) -> Result<Option<AutomationClaimV1>, String> {
    expire_claims(now)?;
    if let Some(claim) = active_claim(now)? {
        return Ok(Some(claim));
    }

    let automations = crate::store::load_automations();
    let recovery = load_ledger()
        .into_iter()
        .filter(|entry| is_lease_interruption(entry))
        .filter(|entry| {
            automations.iter().any(|automation| {
                automation.id == entry.automation_id
                    && automation.enabled
                    && latest_due_slot(automation, now) == Some(entry.scheduled_for)
            })
        })
        .min_by(|left, right| {
            (left.scheduled_for, left.claimed_at, &left.claim_id).cmp(&(
                right.scheduled_for,
                right.claimed_at,
                &right.claim_id,
            ))
        });
    if let Some(recovery) = recovery {
        if let Some(automation) = automations
            .iter()
            .find(|automation| automation.id == recovery.automation_id && automation.enabled)
        {
            let policy = MissedRunPolicyV2::parse(&automation.missed_run_policy);
            let decision = crate::store_lock::update_json_locked(
                &crate::paths::automation_runs_file(),
                Vec::<AutomationRunLedgerEntryV1>::new,
                |ledger| {
                    Ok(claim_occurrence_in_ledger(
                        ledger,
                        &automation.id,
                        recovery.scheduled_for,
                        now,
                        policy,
                        true,
                    ))
                },
            )?;
            return resolve_claim_decision(decision, &automations, automation, now);
        }
    }

    let Some((automation, scheduled_for)) = automations
        .iter()
        .filter(|automation| automation.enabled)
        .filter_map(|automation| {
            latest_due_slot(automation, now).map(|scheduled| (automation, scheduled))
        })
        .min_by_key(|(_, scheduled)| *scheduled)
    else {
        return Ok(None);
    };
    let decision = crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            let policy = MissedRunPolicyV2::parse(&automation.missed_run_policy);
            Ok(claim_occurrence_in_ledger(
                ledger,
                &automation.id,
                scheduled_for,
                now,
                policy,
                false,
            ))
        },
    )?;
    resolve_claim_decision(decision, &automations, automation, now)
}

/// Run one UI-independent scheduler pass using each automation's persisted
/// missed-run policy. The app-resident loop and a future headless runner share
/// this same entry point.
pub fn tick_once() -> Result<Option<AutomationClaimV1>, String> {
    claim_due_once(Utc::now())
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn bind_session_in_ledger(
    ledger: &mut [AutomationRunLedgerEntryV1],
    claim_id: &str,
    session_id: &str,
) -> Result<(), String> {
    let session_id = session_id.trim();
    if !valid_session_id(session_id) {
        return Err("invalid automation session id".into());
    }
    let index = ledger
        .iter()
        .position(|entry| entry.claim_id == claim_id)
        .ok_or_else(|| "automation claim not found".to_string())?;
    if ledger[index].status != AutomationRunStatusV1::Claimed {
        return Err("automation claim already completed".into());
    }
    if ledger[index]
        .session_id
        .as_deref()
        .is_some_and(|known| known != session_id)
    {
        return Err("automation claim is already bound to another session".into());
    }
    if ledger.iter().enumerate().any(|(other_index, entry)| {
        other_index != index
            && entry.status == AutomationRunStatusV1::Claimed
            && entry.session_id.as_deref() == Some(session_id)
    }) {
        return Err("automation session already owns another active claim".into());
    }
    ledger[index].session_id = Some(session_id.to_string());
    Ok(())
}

pub fn bind_session(claim_id: &str, session_id: &str) -> Result<(), String> {
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| bind_session_in_ledger(ledger, claim_id, session_id),
    )
}

fn apply_runtime_heartbeat_v1(
    ledger: &mut [AutomationRunLedgerEntryV1],
    heartbeat: &AutomationRuntimeHeartbeatV1,
    now: DateTime<Utc>,
) -> Result<AutomationRuntimeHeartbeatAckV1, String> {
    if heartbeat.version != 1 {
        return Err("unsupported automation heartbeat version".into());
    }
    if heartbeat.claim_id.is_empty()
        || heartbeat.claim_id.len() > 128
        || heartbeat.claim_id.trim() != heartbeat.claim_id
    {
        return Err("invalid automation claim id".into());
    }
    if !valid_session_id(&heartbeat.session_id)
        || heartbeat.session_id.trim() != heartbeat.session_id
    {
        return Err("invalid automation session id".into());
    }
    if heartbeat.runtime_session_sequence == 0 {
        return Err("runtime session sequence must be positive".into());
    }
    if !valid_process_id(&heartbeat.process_id) || heartbeat.process_id.trim() != heartbeat.process_id
    {
        return Err("invalid automation process id".into());
    }

    let entry = ledger
        .iter_mut()
        .find(|entry| entry.claim_id == heartbeat.claim_id)
        .ok_or_else(|| "automation claim not found".to_string())?;
    if entry.status != AutomationRunStatusV1::Claimed {
        return Err("automation claim is not active".into());
    }
    if entry.lease_until <= now {
        return Err("automation claim lease expired".into());
    }
    if entry.session_id.as_deref() != Some(heartbeat.session_id.as_str()) {
        return Err("automation heartbeat session binding mismatch".into());
    }
    match entry.runtime_process_identity.as_ref() {
        Some(identity)
            if identity.session_id != heartbeat.session_id
                || identity.process_id != heartbeat.process_id =>
        {
            return Err("automation heartbeat process identity mismatch".into());
        }
        Some(_) => {}
        None => {
            entry.runtime_process_identity = Some(AutomationRuntimeProcessIdentityV1 {
                version: 1,
                session_id: heartbeat.session_id.clone(),
                process_id: heartbeat.process_id.clone(),
                first_runtime_session_sequence: heartbeat.runtime_session_sequence,
                bound_at: now,
            });
        }
    }
    if entry
        .last_runtime_session_sequence
        .is_some_and(|known| heartbeat.runtime_session_sequence <= known)
    {
        return Err("runtime session sequence is not increasing".into());
    }

    let lease_until = now + ChronoDuration::minutes(CLAIM_LEASE_MINUTES);
    entry.lease_until = lease_until;
    entry.last_heartbeat_at = Some(now);
    entry.last_runtime_session_sequence = Some(heartbeat.runtime_session_sequence);
    Ok(AutomationRuntimeHeartbeatAckV1 {
        version: 1,
        claim_id: heartbeat.claim_id.clone(),
        session_id: heartbeat.session_id.clone(),
        runtime_session_sequence: heartbeat.runtime_session_sequence,
        recorded_at: now,
        lease_until,
    })
}

/// Extend an active automation lease from monotonic Runtime progress evidence.
/// This is intentionally not exposed as a Tauri command or driven by a UI timer.
pub fn record_runtime_heartbeat_v1(
    heartbeat: AutomationRuntimeHeartbeatV1,
) -> Result<AutomationRuntimeHeartbeatAckV1, String> {
    let now = Utc::now();
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| apply_runtime_heartbeat_v1(ledger, &heartbeat, now),
    )
}

fn active_claim_id_for_heartbeat_session(
    ledger: &[AutomationRunLedgerEntryV1],
    session_id: &str,
) -> Result<Option<String>, String> {
    let matches = ledger
        .iter()
        .filter(|entry| {
            entry.status == AutomationRunStatusV1::Claimed
                && entry.session_id.as_deref() == Some(session_id)
        })
        .map(|entry| entry.claim_id.clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [claim_id] => Ok(Some(claim_id.clone())),
        _ => Err("multiple active automation claims for heartbeat session".into()),
    }
}

/// Resolve a Host-generated Runtime event sequence to the active claim bound
/// to that session. Sessions without an automation claim are a normal no-op.
pub fn record_runtime_heartbeat_for_session_v1(
    session_id: &str,
    runtime_session_sequence: u64,
    process_id: &str,
) -> Result<Option<AutomationRuntimeHeartbeatAckV1>, String> {
    let session_id = session_id.trim();
    if !valid_session_id(session_id) {
        return Err("invalid automation session id".into());
    }
    let Some(claim_id) = active_claim_id_for_heartbeat_session(&load_ledger(), session_id)? else {
        return Ok(None);
    };
    record_runtime_heartbeat_v1(AutomationRuntimeHeartbeatV1 {
        version: 1,
        claim_id,
        session_id: session_id.to_string(),
        process_id: process_id.to_string(),
        runtime_session_sequence,
    })
    .map(Some)
}

fn apply_process_termination_in_ledger(
    ledger: &mut [AutomationRunLedgerEntryV1],
    session_id: &str,
    process_id: &str,
    runtime_session_sequence: u64,
    kind: AutomationTerminationProofKindV1,
    now: DateTime<Utc>,
) -> Result<bool, String> {
    if !valid_session_id(session_id) {
        return Err("invalid automation session id".into());
    }
    if !valid_process_id(process_id) {
        return Err("invalid automation process id".into());
    }
    if runtime_session_sequence == 0 {
        return Err("runtime session sequence must be positive".into());
    }

    let matches = ledger
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            (entry.status == AutomationRunStatusV1::Claimed || is_lease_interruption(entry))
                && entry.session_id.as_deref() == Some(session_id)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let index = match matches.as_slice() {
        [] => return Ok(false),
        [index] => *index,
        _ => return Err("multiple active automation claims for session".into()),
    };
    let entry = &mut ledger[index];
    let Some(identity) = entry.runtime_process_identity.as_ref() else {
        // A termination event cannot establish process identity retroactively.
        return Ok(false);
    };
    if identity.session_id != session_id || identity.process_id != process_id {
        return Err("automation termination process identity mismatch".into());
    }
    if runtime_session_sequence < identity.first_runtime_session_sequence {
        return Err("automation termination sequence precedes process identity".into());
    }
    if let Some(existing) = entry.termination_proof.as_ref() {
        return Ok(existing.process_id == process_id && existing.session_id == session_id);
    }

    entry.termination_proof = Some(AutomationTerminationProofV1 {
        version: 1,
        proof_id: format!("automation-term-v1-{}", Uuid::new_v4()),
        kind,
        claim_id: entry.claim_id.clone(),
        session_id: session_id.to_string(),
        process_id: process_id.to_string(),
        runtime_session_sequence,
        observed_at: now,
    });
    if entry.status == AutomationRunStatusV1::Claimed
        && kind == AutomationTerminationProofKindV1::ProcessExit
    {
        entry.status = AutomationRunStatusV1::Interrupted;
        entry.completed_at = Some(now);
        entry.error = Some(PROCESS_EXITED_ERROR.into());
    }
    Ok(true)
}

/// Record Host-observed Runtime process death for the bound automation claim.
/// This is not a WebView retry token and cannot create a replacement by itself.
pub fn record_process_termination_for_session_v1(
    session_id: &str,
    process_id: &str,
    runtime_session_sequence: u64,
    kind: AutomationTerminationProofKindV1,
) -> Result<bool, String> {
    let now = Utc::now();
    crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| {
            apply_process_termination_in_ledger(
                ledger,
                session_id.trim(),
                process_id.trim(),
                runtime_session_sequence,
                kind,
                now,
            )
        },
    )
}

fn active_claim_id_for_session(
    ledger: &[AutomationRunLedgerEntryV1],
    session_id: &str,
) -> Result<Option<String>, String> {
    let matches = ledger
        .iter()
        .filter(|entry| {
            (entry.status == AutomationRunStatusV1::Claimed || is_lease_interruption(entry))
                && entry.session_id.as_deref() == Some(session_id)
        })
        .map(|entry| entry.claim_id.clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [claim_id] => Ok(Some(claim_id.clone())),
        _ => Err("multiple active automation claims for session".into()),
    }
}

pub fn complete_for_session(
    session_id: &str,
    success: bool,
    error: Option<&str>,
) -> Result<bool, String> {
    let Some(claim_id) = active_claim_id_for_session(&load_ledger(), session_id)? else {
        return Ok(false);
    };
    complete(&claim_id, success, error)?;
    Ok(true)
}

fn complete_entry_in_ledger(
    ledger: &mut Vec<AutomationRunLedgerEntryV1>,
    claim_id: &str,
    success: bool,
    error: Option<&str>,
    now: DateTime<Utc>,
) -> Result<String, String> {
    let entry = ledger
        .iter_mut()
        .find(|entry| entry.claim_id == claim_id)
        .ok_or_else(|| "automation claim not found".to_string())?;
    if entry.replacement_claim_id.is_some() {
        return Err("replaced automation claim cannot accept a late completion".into());
    }
    if entry.status != AutomationRunStatusV1::Claimed && !is_lease_interruption(entry) {
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
    let automation_id = entry.automation_id.clone();
    trim_ledger(ledger);
    Ok(automation_id)
}

pub fn complete(claim_id: &str, success: bool, error: Option<&str>) -> Result<(), String> {
    let now = Utc::now();
    let automation_id = crate::store_lock::update_json_locked(
        &crate::paths::automation_runs_file(),
        Vec::<AutomationRunLedgerEntryV1>::new,
        |ledger| complete_entry_in_ledger(ledger, claim_id, success, error, now),
    )?;
    let automation = crate::store::load_automations()
        .into_iter()
        .find(|automation| automation.id == automation_id)
        .ok_or_else(|| "automation not found".to_string())?;

    // claimId is the CAS boundary. A later tick can retry schedule advancement,
    // but a repeated completion can never overwrite the terminal result.
    advance_after_occurrence(&automation, now)
}

fn tick(app: &AppHandle) {
    match tick_once() {
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
            missed_run_policy: "run_once".into(),
            notify: "none".into(),
            created_at: now,
            updated_at: now,
            last_run_at: None,
            next_run_at: None,
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 24, 12, 0, 0)
            .single()
            .unwrap()
    }

    fn claimed_entry(decision: LedgerClaimDecision) -> AutomationRunLedgerEntryV1 {
        match decision {
            LedgerClaimDecision::Claimed(entry) => entry,
            _ => panic!("expected a newly claimed occurrence"),
        }
    }

    fn heartbeat(
        claim_id: &str,
        session_id: &str,
        runtime_session_sequence: u64,
    ) -> AutomationRuntimeHeartbeatV1 {
        AutomationRuntimeHeartbeatV1 {
            version: 1,
            claim_id: claim_id.into(),
            session_id: session_id.into(),
            process_id: "proc-1".into(),
            runtime_session_sequence,
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

    #[test]
    fn duplicate_claim_returns_the_same_active_claim_without_a_second_ledger_row() {
        let now = fixed_now();
        let mut ledger = Vec::new();

        let first = claimed_entry(claim_occurrence_in_ledger(
            &mut ledger,
            "duplicate",
            now,
            now,
            MissedRunPolicyV2::RunOnce,
            false,
        ));
        let second = claim_occurrence_in_ledger(
            &mut ledger,
            "duplicate",
            now,
            now,
            MissedRunPolicyV2::RunOnce,
            false,
        );
        let LedgerClaimDecision::Existing(second) = second else {
            panic!("second claim should observe the existing lease");
        };

        assert_eq!(second.claim_id, first.claim_id);
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn legacy_ledger_rows_without_heartbeat_fields_remain_readable() {
        let now = fixed_now();
        let entry = new_claim_entry("legacy", now, now, false);
        let mut value = serde_json::to_value(entry).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("lastHeartbeatAt");
        object.remove("lastRuntimeSessionSequence");

        let decoded: AutomationRunLedgerEntryV1 = serde_json::from_value(value).unwrap();
        assert!(decoded.last_heartbeat_at.is_none());
        assert!(decoded.last_runtime_session_sequence.is_none());
    }

    #[test]
    fn runtime_heartbeat_requires_monotonic_evidence_and_bounds_the_lease() {
        let now = fixed_now();
        let mut claim = new_claim_entry("heartbeat", now, now - ChronoDuration::minutes(1), false);
        claim.claim_id = "heartbeat-claim".into();
        claim.session_id = Some("runtime-session".into());
        let mut ledger = vec![claim];

        let first = apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 41),
            now,
        )
        .unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(first.runtime_session_sequence, 41);
        assert_eq!(
            first.lease_until,
            now + ChronoDuration::minutes(CLAIM_LEASE_MINUTES)
        );
        assert_eq!(ledger[0].last_heartbeat_at, Some(now));
        assert_eq!(ledger[0].last_runtime_session_sequence, Some(41));

        let next_time = now + ChronoDuration::minutes(1);
        apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 42),
            next_time,
        )
        .unwrap();
        let accepted_lease = ledger[0].lease_until;
        assert_eq!(ledger[0].last_heartbeat_at, Some(next_time));
        assert_eq!(ledger[0].last_runtime_session_sequence, Some(42));

        for sequence in [42, 40] {
            let error = apply_runtime_heartbeat_v1(
                &mut ledger,
                &heartbeat("heartbeat-claim", "runtime-session", sequence),
                next_time + ChronoDuration::seconds(1),
            )
            .unwrap_err();
            assert!(error.contains("not increasing"));
        }
        assert_eq!(ledger[0].lease_until, accepted_lease);
        assert_eq!(ledger[0].last_runtime_session_sequence, Some(42));
    }

    #[test]
    fn runtime_heartbeat_rejects_wrong_binding_and_never_revives_dead_claims() {
        let now = fixed_now();
        let mut claim = new_claim_entry("heartbeat", now, now, false);
        claim.claim_id = "heartbeat-claim".into();
        let mut ledger = vec![claim];

        let mut unsupported = heartbeat("heartbeat-claim", "runtime-session", 1);
        unsupported.version = 2;
        assert!(apply_runtime_heartbeat_v1(&mut ledger, &unsupported, now)
            .unwrap_err()
            .contains("unsupported"));
        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 0),
            now,
        )
        .unwrap_err()
        .contains("must be positive"));

        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 1),
            now,
        )
        .unwrap_err()
        .contains("binding mismatch"));
        ledger[0].session_id = Some("runtime-session".into());
        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "other-session", 1),
            now,
        )
        .unwrap_err()
        .contains("binding mismatch"));

        ledger[0].lease_until = now;
        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 1),
            now,
        )
        .unwrap_err()
        .contains("lease expired"));
        assert!(ledger[0].last_heartbeat_at.is_none());

        expire_claims_in_ledger(&mut ledger, now);
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 1),
            now,
        )
        .unwrap_err()
        .contains("not active"));
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
        assert!(ledger[0].last_runtime_session_sequence.is_none());

        ledger[0].status = AutomationRunStatusV1::Succeeded;
        assert!(apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("heartbeat-claim", "runtime-session", 2),
            now,
        )
        .unwrap_err()
        .contains("not active"));
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Succeeded);
    }

    #[test]
    fn heartbeat_session_lookup_ignores_interrupted_and_terminal_claims() {
        let now = fixed_now();
        let mut active = new_claim_entry("heartbeat", now, now, false);
        active.claim_id = "active".into();
        active.session_id = Some("runtime-session".into());
        let mut interrupted = active.clone();
        interrupted.claim_id = "interrupted".into();
        interrupted.status = AutomationRunStatusV1::Interrupted;
        interrupted.error = Some(LEASE_EXPIRED_ERROR.into());
        let mut succeeded = active.clone();
        succeeded.claim_id = "succeeded".into();
        succeeded.status = AutomationRunStatusV1::Succeeded;

        assert_eq!(
            active_claim_id_for_heartbeat_session(
                &[interrupted.clone(), succeeded, active.clone()],
                "runtime-session",
            )
            .unwrap(),
            Some("active".into()),
        );
        assert_eq!(
            active_claim_id_for_heartbeat_session(&[interrupted], "runtime-session").unwrap(),
            None,
        );

        let duplicate = vec![active.clone(), active];
        assert!(
            active_claim_id_for_heartbeat_session(&duplicate, "runtime-session")
                .unwrap_err()
                .contains("multiple active")
        );
    }

    #[test]
    fn expired_claim_is_interrupted_without_a_replacement_claim() {
        let now = fixed_now();
        let scheduled_for = now - ChronoDuration::minutes(5);
        let mut crashed = new_claim_entry(
            "recover",
            scheduled_for,
            now - ChronoDuration::minutes(20),
            false,
        );
        crashed.claim_id = "crashed-claim".into();
        let mut ledger = vec![crashed];

        expire_claims_in_ledger(&mut ledger, now);
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut ledger,
                "recover",
                scheduled_for,
                now,
                MissedRunPolicyV2::RunOnce,
                true,
            ),
            LedgerClaimDecision::Suppressed
        ));
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].claim_id, "crashed-claim");
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
    }

    #[test]
    fn heartbeat_binds_process_identity_and_termination_proof_cannot_create_it() {
        let now = fixed_now();
        let mut claim = new_claim_entry("identity", now, now, false);
        claim.claim_id = "identity-claim".into();
        claim.session_id = Some("runtime-session".into());
        let mut ledger = vec![claim];

        apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("identity-claim", "runtime-session", 3),
            now,
        )
        .unwrap();
        let identity = ledger[0].runtime_process_identity.clone().unwrap();
        assert_eq!(identity.process_id, "proc-1");
        assert_eq!(identity.first_runtime_session_sequence, 3);

        let mut unbound = new_claim_entry("unbound", now, now, false);
        unbound.claim_id = "unbound-claim".into();
        unbound.session_id = Some("runtime-session".into());
        let mut unbound_ledger = vec![unbound];
        assert_eq!(
            apply_process_termination_in_ledger(
                &mut unbound_ledger,
                "runtime-session",
                "proc-1",
                3,
                AutomationTerminationProofKindV1::ProcessExit,
                now,
            )
            .unwrap(),
            false
        );
        assert!(unbound_ledger[0].termination_proof.is_none());
        assert_eq!(unbound_ledger[0].status, AutomationRunStatusV1::Claimed);
    }

    #[test]
    fn process_exit_proof_authorizes_exactly_one_replacement() {
        let now = fixed_now();
        let scheduled_for = now - ChronoDuration::minutes(5);
        let mut claim = new_claim_entry("recover", scheduled_for, now, false);
        claim.claim_id = "crashed-claim".into();
        claim.session_id = Some("runtime-session".into());
        let mut ledger = vec![claim];
        apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("crashed-claim", "runtime-session", 1),
            now,
        )
        .unwrap();
        assert!(apply_process_termination_in_ledger(
            &mut ledger,
            "runtime-session",
            "proc-1",
            4,
            AutomationTerminationProofKindV1::ProcessExit,
            now + ChronoDuration::seconds(1),
        )
        .unwrap());
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
        assert!(ledger[0].termination_proof.is_some());

        let replacement = claimed_entry(claim_occurrence_in_ledger(
            &mut ledger,
            "recover",
            scheduled_for,
            now + ChronoDuration::minutes(1),
            MissedRunPolicyV2::RunOnce,
            true,
        ));
        assert_eq!(ledger.len(), 2);
        assert_eq!(replacement.replaces_claim_id.as_deref(), Some("crashed-claim"));
        assert_eq!(
            ledger[0].replacement_claim_id.as_deref(),
            Some(replacement.claim_id.as_str())
        );
        assert_eq!(ledger[1].status, AutomationRunStatusV1::Claimed);

        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut ledger,
                "recover",
                scheduled_for,
                now + ChronoDuration::minutes(2),
                MissedRunPolicyV2::RunOnce,
                true,
            ),
            LedgerClaimDecision::Existing(_)
        ));
        assert_eq!(ledger.len(), 2);
    }

    #[test]
    fn skip_policy_and_missing_proof_never_replace() {
        let now = fixed_now();
        let scheduled_for = now - ChronoDuration::minutes(5);
        let mut with_proof = new_claim_entry("skip-me", scheduled_for, now, false);
        with_proof.claim_id = "skip-claim".into();
        with_proof.session_id = Some("runtime-session".into());
        let mut ledger = vec![with_proof];
        apply_runtime_heartbeat_v1(
            &mut ledger,
            &heartbeat("skip-claim", "runtime-session", 1),
            now,
        )
        .unwrap();
        apply_process_termination_in_ledger(
            &mut ledger,
            "runtime-session",
            "proc-1",
            2,
            AutomationTerminationProofKindV1::ProcessExit,
            now,
        )
        .unwrap();
        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut ledger,
                "skip-me",
                scheduled_for,
                now,
                MissedRunPolicyV2::Skip,
                true,
            ),
            LedgerClaimDecision::Suppressed
        ));
        assert_eq!(ledger.len(), 1);
        assert!(ledger[0].replacement_claim_id.is_none());
    }

    #[test]
    fn replaced_original_rejects_late_completion() {
        let now = fixed_now();
        let scheduled_for = now - ChronoDuration::minutes(5);
        let mut claim = new_claim_entry("recover", scheduled_for, now, false);
        claim.claim_id = "original".into();
        claim.session_id = Some("runtime-session".into());
        let mut ledger = vec![claim];
        apply_runtime_heartbeat_v1(&mut ledger, &heartbeat("original", "runtime-session", 1), now)
            .unwrap();
        apply_process_termination_in_ledger(
            &mut ledger,
            "runtime-session",
            "proc-1",
            2,
            AutomationTerminationProofKindV1::ProcessExit,
            now,
        )
        .unwrap();
        let _ = claimed_entry(claim_occurrence_in_ledger(
            &mut ledger,
            "recover",
            scheduled_for,
            now,
            MissedRunPolicyV2::RunOnce,
            true,
        ));
        let late = complete_entry_in_ledger(&mut ledger, "original", true, None, now)
            .unwrap_err();
        assert!(late.contains("replaced"));
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Interrupted);
    }

    #[test]
    fn late_completion_settles_only_the_expired_original_and_is_idempotent() {
        let now = fixed_now();
        let mut claim = new_claim_entry(
            "long-running",
            now - ChronoDuration::minutes(20),
            now - ChronoDuration::minutes(20),
            false,
        );
        claim.claim_id = "original-claim".into();
        claim.session_id = Some("long-session".into());
        let mut ledger = vec![claim];
        expire_claims_in_ledger(&mut ledger, now);

        assert_eq!(
            active_claim_id_for_session(&ledger, "long-session").unwrap(),
            Some("original-claim".into())
        );
        complete_entry_in_ledger(
            &mut ledger,
            "original-claim",
            true,
            None,
            now + ChronoDuration::minutes(1),
        )
        .unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].claim_id, "original-claim");
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Succeeded);

        let duplicate = complete_entry_in_ledger(
            &mut ledger,
            "original-claim",
            false,
            Some("late duplicate"),
            now + ChronoDuration::minutes(2),
        )
        .unwrap_err();
        assert!(duplicate.contains("already completed"));
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Succeeded);
        assert!(ledger[0].error.is_none());
    }

    #[test]
    fn missed_run_policy_skips_or_claims_exactly_once() {
        let now = fixed_now();
        let scheduled_for = now - ChronoDuration::minutes(5);
        assert_eq!(MissedRunPolicyV2::parse("skip"), MissedRunPolicyV2::Skip);
        assert_eq!(
            MissedRunPolicyV2::parse("unknown"),
            MissedRunPolicyV2::RunOnce
        );

        let mut skip = Vec::new();
        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut skip,
                "skip",
                scheduled_for,
                now,
                MissedRunPolicyV2::Skip,
                false,
            ),
            LedgerClaimDecision::Suppressed
        ));
        assert_eq!(skip.len(), 1);
        assert_eq!(skip[0].status, AutomationRunStatusV1::Skipped);
        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut skip,
                "skip",
                scheduled_for,
                now,
                MissedRunPolicyV2::Skip,
                false,
            ),
            LedgerClaimDecision::Suppressed
        ));
        assert_eq!(skip.len(), 1);

        let mut run_once = Vec::new();
        let claim = claimed_entry(claim_occurrence_in_ledger(
            &mut run_once,
            "run-once",
            scheduled_for,
            now,
            MissedRunPolicyV2::RunOnce,
            false,
        ));
        assert!(claim.catch_up);
        assert_eq!(run_once.len(), 1);
        assert_eq!(MissedRunPolicyV2::default(), MissedRunPolicyV2::RunOnce);

        let mut expired = new_claim_entry(
            "skip-recovery",
            scheduled_for,
            now - ChronoDuration::minutes(20),
            false,
        );
        expired.lease_until = now - ChronoDuration::seconds(1);
        let mut recovery_ledger = vec![expired];
        expire_claims_in_ledger(&mut recovery_ledger, now);
        assert!(matches!(
            claim_occurrence_in_ledger(
                &mut recovery_ledger,
                "skip-recovery",
                scheduled_for,
                now,
                MissedRunPolicyV2::Skip,
                true,
            ),
            LedgerClaimDecision::Suppressed
        ));
        assert_eq!(recovery_ledger.len(), 1);
    }

    #[test]
    fn duplicate_completion_is_compare_and_set_and_fails_closed() {
        let now = fixed_now();
        let claim = new_claim_entry("complete", now, now, false);
        let claim_id = claim.claim_id.clone();
        let mut ledger = vec![claim];

        complete_entry_in_ledger(
            &mut ledger,
            &claim_id,
            true,
            None,
            now + ChronoDuration::minutes(1),
        )
        .unwrap();
        let duplicate = complete_entry_in_ledger(
            &mut ledger,
            &claim_id,
            false,
            Some("must not overwrite success"),
            now + ChronoDuration::minutes(2),
        )
        .unwrap_err();

        assert!(duplicate.contains("already completed"));
        assert_eq!(ledger[0].status, AutomationRunStatusV1::Succeeded);
        assert!(ledger[0].error.is_none());
    }

    #[test]
    fn session_binding_is_idempotent_and_isolated_across_claims() {
        let now = fixed_now();
        let first = new_claim_entry("session", now, now, false);
        let first_id = first.claim_id.clone();
        let second = new_claim_entry("session", now + ChronoDuration::days(1), now, false);
        let second_id = second.claim_id.clone();
        let mut ledger = vec![first, second];

        bind_session_in_ledger(&mut ledger, &first_id, "session-a").unwrap();
        bind_session_in_ledger(&mut ledger, &first_id, "session-a").unwrap();
        assert!(bind_session_in_ledger(&mut ledger, &first_id, "session-b")
            .unwrap_err()
            .contains("another session"));
        assert!(bind_session_in_ledger(&mut ledger, &second_id, "session-a")
            .unwrap_err()
            .contains("already owns"));

        assert_eq!(
            active_claim_id_for_session(&ledger, "session-b").unwrap(),
            None
        );
        assert_eq!(
            active_claim_id_for_session(&ledger, "session-a").unwrap(),
            Some(first_id.clone())
        );
        complete_entry_in_ledger(
            &mut ledger,
            &first_id,
            true,
            None,
            now + ChronoDuration::minutes(1),
        )
        .unwrap();
        assert_eq!(
            ledger
                .iter()
                .find(|entry| entry.claim_id == second_id)
                .unwrap()
                .status,
            AutomationRunStatusV1::Claimed
        );
    }

    #[test]
    fn ledger_pruning_keeps_active_claims_within_the_bound() {
        let now = fixed_now();
        let mut ledger = (0..MAX_LEDGER_ROWS + 8)
            .map(|offset| {
                let mut entry = new_claim_entry(
                    "old",
                    now - ChronoDuration::minutes(offset as i64),
                    now - ChronoDuration::minutes(offset as i64),
                    false,
                );
                entry.status = AutomationRunStatusV1::Succeeded;
                entry.completed_at = Some(now);
                entry
            })
            .collect::<Vec<_>>();
        let mut active = new_claim_entry("active", now, now, false);
        active.claim_id = "keep-active".into();
        ledger.push(active);

        trim_ledger(&mut ledger);

        assert_eq!(ledger.len(), MAX_LEDGER_ROWS);
        assert!(ledger.iter().any(|entry| {
            entry.claim_id == "keep-active" && entry.status == AutomationRunStatusV1::Claimed
        }));
    }
}
