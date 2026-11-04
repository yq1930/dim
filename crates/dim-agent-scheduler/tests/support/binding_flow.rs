use crate::support;
use dim_client::Client;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

static PAUSES: Mutex<BTreeMap<String, mpsc::UnboundedSender<oneshot::Sender<()>>>> =
    Mutex::new(BTreeMap::new());

pub(crate) async fn pause(job_id: &str) {
    let sender = PAUSES.lock().unwrap().get(job_id).cloned();
    if let Some(sender) = sender {
        let (release, released) = oneshot::channel();
        if sender.send(release).is_ok() {
            tokio::time::timeout(Duration::from_secs(30), released)
                .await
                .expect("the acknowledged binding attempt is released")
                .ok();
        }
    }
}

struct BindingPause {
    job_id: String,
    attempts: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
}

impl BindingPause {
    fn new(job_id: &str) -> Self {
        let (sender, attempts) = mpsc::unbounded_channel();
        assert!(
            PAUSES
                .lock()
                .unwrap()
                .insert(job_id.into(), sender)
                .is_none()
        );
        Self {
            job_id: job_id.into(),
            attempts,
        }
    }

    async fn next(&mut self) -> oneshot::Sender<()> {
        tokio::time::timeout(Duration::from_secs(30), self.attempts.recv())
            .await
            .expect("binding reaches the snapshot/CAS boundary")
            .expect("binding acknowledgement channel stays open")
    }
}

impl Drop for BindingPause {
    fn drop(&mut self) {
        PAUSES.lock().unwrap().remove(&self.job_id);
    }
}

#[derive(Clone, Copy, Debug)]
enum Change {
    OneConflict,
    TwoConflicts,
    Disable,
    Revise,
}

fn stored_settings(home: &std::path::Path) -> Value {
    let connection = rusqlite::Connection::open_with_flags(
        home.join("dimcode.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (revision, document): (i64, String) = connection
        .query_row(
            "SELECT version, document FROM settings_documents WHERE source = 'user/schedule'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    json!({"revision": revision, "document": serde_json::from_str::<Value>(&document).unwrap()})
}

async fn assert_deliveries(client: &mut Client, runs: &[Value]) {
    let catalog = client
        .rpc(
            "session.catalog",
            json!({"withInternal": true, "limit": 100}),
        )
        .await
        .unwrap();
    assert!(catalog["nextCursor"].is_null(), "{catalog}");
    let sessions: BTreeSet<_> = catalog["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["sessionId"].as_str().unwrap())
        .collect();
    let expected: BTreeSet<_> = runs
        .iter()
        .map(|run| run["session_id"].as_str().unwrap())
        .collect();
    assert_eq!(sessions, expected, "no extra launch");
    for session_id in sessions {
        let history = client
            .rpc(
                "session.history",
                json!({"sessionId": session_id, "branchId": "main", "limit": 4096}),
            )
            .await
            .unwrap();
        assert_eq!(history["done"], true, "the entire short history: {history}");
        let inputs: Vec<_> = history["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| {
                event["type"] == "input"
                    && event["name"] == "message"
                    && event["data"]["submission_id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("scheduler:"))
            })
            .collect();
        let submitted: Vec<_> = runs
            .iter()
            .filter(|run| run["session_id"] == session_id && run["input_event_id"].is_string())
            .collect();
        assert_eq!(inputs.len(), submitted.len(), "{history}");
        for run in submitted {
            let matching: Vec<_> = inputs
                .iter()
                .filter(|event| event["data"]["submission_id"] == run["submission_id"])
                .collect();
            assert_eq!(matching.len(), 1, "exactly one input for {run}");
            assert_eq!(matching[0]["id"], run["input_event_id"], "{run}");
        }
    }
}

async fn binding_flow(change: Change) {
    tokio::time::timeout(Duration::from_secs(90), async {
        let home = support::home();
        let model = Arc::new(support::Script::default());
        let server = support::start(home.path(), model.clone()).await;
        let mut client = support::connect(&server).await;
        let saved: Value = client
            .typed(
                "schedule.save",
                json!({"job": {"name": "binding target", "mode": "reuse",
                    "schedule": {"at": "2030-01-01T00:00:00Z"},
                    "prompt": "Check the binding target.", "cwd": home.path()}}),
            )
            .await
            .unwrap();
        let original = saved["job"].clone();
        let job_id = original["id"].as_str().unwrap();
        let unrelated: Value = client
            .typed(
                "schedule.save",
                json!({"job": {"name": "unrelated", "schedule": {"at": "2031-01-01T00:00:00Z"},
                    "prompt": "Unrelated work.", "cwd": home.path()}}),
            )
            .await
            .unwrap();
        let mut unrelated = unrelated["job"].clone();
        let mut gate = BindingPause::new(job_id);
        let accepted: Value = client
            .typed("schedule.run_now", json!({"job_id": job_id}))
            .await
            .unwrap();
        let release = gate.next().await;
        let launching = support::run_where(
            &mut client,
            job_id,
            accepted["run_id"].as_str().unwrap(),
            |run| run["session_id"].is_string(),
        )
        .await;
        assert_eq!(launching["state"], "launching", "{launching}");
        assert_eq!(launching["session_origin"], "launch", "{launching}");
        assert!(launching["input_event_id"].is_null(), "{launching}");
        assert_deliveries(&mut client, std::slice::from_ref(&launching)).await;

        let attempts = if matches!(change, Change::OneConflict | Change::TwoConflicts) {
            2
        } else {
            1
        };
        let mut release = release;
        let mut selected = original.clone();
        for attempt in 1..=attempts {
            let before = stored_settings(home.path());
            if matches!(change, Change::OneConflict) && attempt == 2 {
                eprintln!("{change:?}: retry acknowledged after the first definitive conflict");
            } else {
                let updated: Value = match change {
                    Change::OneConflict | Change::TwoConflicts => {
                        client
                            .typed(
                                "schedule.save",
                                json!({"job": {"id": unrelated["id"],
                                    "name": format!("unrelated edit {attempt}")},
                                    "revision": unrelated["revision"]}),
                            )
                            .await
                            .unwrap()
                    }
                    Change::Disable => {
                        client
                            .typed(
                                "schedule.set_enabled",
                                json!({"job_id": job_id, "enabled": false}),
                            )
                            .await
                            .unwrap()
                    }
                    Change::Revise => {
                        client
                            .typed(
                                "schedule.save",
                                json!({"job": {"id": job_id, "prompt": "Changed prompt."},
                                    "revision": original["revision"]}),
                            )
                            .await
                            .unwrap()
                    }
                };
                match change {
                    Change::OneConflict | Change::TwoConflicts => unrelated = updated["job"].clone(),
                    Change::Disable | Change::Revise => selected = updated["job"].clone(),
                }
                let after = stored_settings(home.path());
                assert_ne!(before["revision"], after["revision"], "a committed CAS invalidation");
                eprintln!(
                    "{change:?}: attempt {attempt} snapshot acknowledged; committed revision {} -> {} before release",
                    before["revision"], after["revision"]
                );
            }
            if attempt == attempts {
                drop(gate);
                release.send(()).expect("binding attempt is still paused");
                break;
            }
            release.send(()).expect("binding attempt is still paused");
            release = gate.next().await;
        }

        let first = support::settled(&mut client, job_id, accepted["run_id"].as_str().unwrap()).await;
        assert_eq!(first["session_id"], launching["session_id"], "no relaunch: {first}");
        let snapshot = stored_settings(home.path());
        let jobs = snapshot["document"]["jobs"].as_array().unwrap();
        assert_eq!(jobs.iter().find(|job| job["id"] == unrelated["id"]).unwrap(), &unrelated);
        let job = jobs.iter().find(|job| job["id"] == job_id).unwrap();
        match change {
            Change::OneConflict => {
                selected["session_id"] = first["session_id"].clone();
                selected["revision"] = json!(2);
                assert_eq!(first["state"], "completed", "{first}");
                assert_eq!(first["detail"], "completed", "{first}");
            }
            Change::TwoConflicts => {
                assert_eq!(first["state"], "completed", "{first}");
                assert_eq!(first["detail"], "binding_conflict; completed", "{first}");
            }
            Change::Disable | Change::Revise => {
                assert_eq!(first["state"], "skipped", "{first}");
                assert_eq!(first["detail"], if matches!(change, Change::Disable) { "disabled" } else { "changed" }, "{first}");
                assert!(first["input_event_id"].is_null(), "{first}");
            }
        }
        assert_eq!(job, &selected, "binding preserves every other business field");
        let mut runs = vec![first];
        if matches!(change, Change::OneConflict | Change::TwoConflicts) {
            let accepted: Value = client
                .typed("schedule.run_now", json!({"job_id": job_id}))
                .await
                .unwrap();
            let next = support::settled(&mut client, job_id, accepted["run_id"].as_str().unwrap()).await;
            assert_eq!(next["state"], "completed", "{next}");
            assert_eq!(next["detail"], "completed", "{next}");
            if matches!(change, Change::OneConflict) {
                assert_eq!(next["session_origin"], "reuse", "{next}");
                assert_eq!(next["session_id"], runs[0]["session_id"], "{next}");
            } else {
                assert_eq!(next["session_origin"], "launch", "{next}");
                assert_ne!(next["session_id"], runs[0]["session_id"], "{next}");
            }
            selected["session_id"] = next["session_id"].clone();
            selected["revision"] = json!(2);
            let snapshot = stored_settings(home.path());
            assert_eq!(snapshot["document"]["jobs"].as_array().unwrap().iter().find(|job| job["id"] == job_id).unwrap(), &selected);
            runs.push(next);
        }
        assert_deliveries(&mut client, &runs).await;
        let expected_inputs = if matches!(change, Change::OneConflict | Change::TwoConflicts) { 2 } else { 0 };
        assert_eq!(model.count(), expected_inputs);
        eprintln!("{change:?}: verified histories={}", json!(runs));
        drop(client);
        server.shutdown();
        server.join().await.unwrap();
    })
    .await
    .expect("the public binding flow finishes within its deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_definitive_conflict_retries_and_binds_before_one_input() {
    binding_flow(Change::OneConflict).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_definitive_conflicts_input_once_unbound_and_the_next_run_launches_and_binds() {
    binding_flow(Change::TwoConflicts).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disabled_job_after_the_first_snapshot_skips_without_input() {
    binding_flow(Change::Disable).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revised_job_after_the_first_snapshot_skips_without_input() {
    binding_flow(Change::Revise).await;
}
