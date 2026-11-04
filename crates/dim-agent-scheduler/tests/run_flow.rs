//! A manual run (`schedule.run_now`) through a real App Server: the run
//! launches a non-interactive Session, its scheduled-run input is consumed,
//! and the run settles `completed` with its effective authority recorded.
mod support;

use dim_agent_capabilities::{defaults, history::HistoryRenderer, host::*};
use dim_agent_runtime::{AgentBuilder, AgentConfig};
use dim_agent_scheduler::install;
use dim_agent_types::*;
use dim_app_server::{ServerConfig, serve};
use dim_client::{Client, ConnectionConfig, Server};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Replies with queued tool batches, then with text; keeps every request.
#[derive(Default)]
struct Script {
    batches: Mutex<VecDeque<Vec<ToolCall>>>,
    requests: Mutex<Vec<Arc<ModelRequest>>>,
}

impl Script {
    fn texts(&self) -> String {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| format!("{request:?}"))
            .collect()
    }
}

impl ModelDriver for Script {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        _: ModelConnection,
        _: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            let id = format!("m{}", self.requests.lock().unwrap().len());
            let mut chunks = vec![Ok(ModelChunk::MessageStart {
                message_id: id.clone(),
            })];
            let reason = match self.batches.lock().unwrap().pop_front() {
                Some(calls) => {
                    for call in calls {
                        chunks.push(Ok(ModelChunk::ToolStart {
                            id: call.id.clone(),
                            name: call.name.clone(),
                        }));
                        chunks.push(Ok(ModelChunk::ToolUse(call)));
                    }
                    ModelStopReason::ToolUse
                }
                None => {
                    chunks.push(Ok(ModelChunk::Text("done".into())));
                    chunks.push(Ok(ModelChunk::TextEnd));
                    ModelStopReason::EndTurn
                }
            };
            chunks.push(Ok(ModelChunk::MessageStop {
                message_id: id,
                reason,
            }));
            Ok(Box::pin(futures_util::stream::iter(chunks)) as ModelStream)
        })
    }
}

/// A selected model, as the App Server's profile needs before a Session runs.
/// The scripted model answers every request, so the endpoint is never called.
async fn select_model(client: &mut Client) {
    client
        .rpc(
            "provider.create",
            json!({"providerId": "018ec73f-0000-7000-8000-000000000007", "displayName": "Fixture",
                "connection": {"driverKind": "openai-compatible", "baseUrl": "http://localhost/v1",
                    "auth": {"type": "anonymous"}},
                "models": [{"modelId": "fixture", "displayName": "Fixture", "status": "active",
                    "capabilities": dim_agent_types::CatalogCapabilities::with_limits(128_000, 16_384)}],
                "enabled": true}),
        )
        .await
        .unwrap();
    client
        .rpc(
            "provider.select",
            json!({"providerId": "018ec73f-0000-7000-8000-000000000007", "modelId": "fixture"}),
        )
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_manual_run_launches_a_session_and_settles_completed() {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir_all(home.path().join("scheduler")).unwrap();

    let model = Arc::new(Script::default());
    let mut builder = AgentBuilder::new(AgentConfig::default());
    defaults::install(&mut builder);
    defaults::mock_hosts_with_model(&mut builder, HistoryRenderer::default(), model.clone());
    install(&mut builder);
    let server = serve(
        builder,
        ServerConfig {
            startup_timeout: Duration::from_secs(60),
            database_path: Some(home.path().join("dimcode.sqlite")),
            product_home: Some(home.path().to_owned()),
            unix_path: Some(home.path().join("native.sock")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let config = ConnectionConfig {
        endpoint: server.unix_path.clone().unwrap(),
        server: Server {
            program: std::env::current_exe().unwrap(),
            embedded: false,
        },
        profile: None,
        client_kind: "cli",
        client_name: "scheduler-run-test",
    };
    let mut client = Client::connect(&config).await.unwrap();
    select_model(&mut client).await;

    let saved: Value = client
        .typed(
            "schedule.save",
            json!({"job": {"name": "nightly", "schedule": {"at": "2030-01-01T00:00:00Z"},
                "prompt": "Summarize the day.", "cwd": home.path()}}),
        )
        .await
        .unwrap();
    let job_id = saved["job"]["id"].as_str().unwrap().to_owned();

    let accepted: Value = client
        .typed("schedule.run_now", json!({"job_id": job_id}))
        .await
        .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap().to_owned();
    assert_eq!(accepted["state"], "launching", "{accepted}");

    let run = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let history: Value = client
                .typed("schedule.history", json!({"job_id": job_id}))
                .await
                .unwrap();
            let found = history["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|run| run["run_id"] == run_id.as_str())
                .cloned();
            if let Some(run) = found
                && !matches!(
                    run["state"].as_str(),
                    Some("launching" | "submitted" | "running")
                )
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the run settles");
    assert_eq!(run["state"], "completed", "{run}");
    assert_eq!(run["detail"], "completed", "{run}");
    assert_eq!(run["session_origin"], "launch", "{run}");
    assert!(run["session_id"].is_string(), "{run}");
    assert!(run["input_event_id"].is_string(), "{run}");
    assert_eq!(run["policy"]["preset"], "read-only", "{run}");

    let requests = model.texts();
    assert!(
        requests.contains("scheduled-run"),
        "the marker reached the model"
    );
    assert!(
        requests.contains("Summarize the day."),
        "the prompt reached the model"
    );

    drop(client);
    server.shutdown();
    server.join().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_first_reuse_runs_input_once_and_later_follow_the_settings_binding() {
    let home = support::home();
    let model = Arc::new(support::Script::default());
    model.hold("scheduled-run");
    let server = support::start(home.path(), model.clone()).await;
    let mut client = support::connect(&server).await;
    let mut jobs = Vec::new();
    let mut callers = Vec::new();
    for index in 0..8 {
        let saved: Value = client
            .typed(
                "schedule.save",
                json!({"job": {"name": format!("concurrent inbox {index}"),
                    "schedule": {"at": "2030-01-01T00:00:00Z"},
                    "prompt": format!("Check concurrent inbox {index}."),
                    "cwd": home.path(), "mode": "reuse"}}),
            )
            .await
            .unwrap();
        assert!(saved["job"]["session_id"].is_null(), "{saved}");
        jobs.push(saved["job"]["id"].as_str().unwrap().to_owned());
        callers.push(support::connect(&server).await);
    }

    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let accepted = futures_util::future::join_all(callers.into_iter().zip(&jobs).map(
        |(mut caller, job_id)| {
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                let run: Value = caller
                    .typed("schedule.run_now", json!({"job_id": job_id}))
                    .await
                    .unwrap();
                assert_eq!(run["state"], "launching", "{run}");
                run
            }
        },
    ))
    .await;
    support::wait_for("all eight scheduled inputs reach the model", || {
        model.count() == 8
    })
    .await;
    model.release();

    let mut runs = Vec::new();
    let mut unbound = Vec::new();
    let mut expected_sessions = std::collections::BTreeSet::new();
    for (job_id, accepted) in jobs.iter().zip(&accepted) {
        let run = support::settled(&mut client, job_id, accepted["run_id"].as_str().unwrap()).await;
        assert_eq!(run["state"], "completed", "{run}");
        assert_eq!(run["session_origin"], "launch", "{run}");
        assert!(
            expected_sessions.insert(run["session_id"].as_str().unwrap().to_owned()),
            "each first run launches a distinct Session: {run}"
        );
        let listed: Value = client.typed("schedule.list", json!({})).await.unwrap();
        let job = listed["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == *job_id)
            .unwrap();
        let conflict = run["detail"] == "binding_conflict; completed";
        if conflict {
            assert!(job["session_id"].is_null(), "{job}");
            assert_eq!(job["revision"], 1, "{job}");
        } else {
            assert_eq!(run["detail"], "completed", "{run}");
            assert_eq!(job["session_id"], run["session_id"], "{job}");
            assert_eq!(job["revision"], 2, "{job}");
        }
        unbound.push(conflict);
        runs.push(run);
    }
    eprintln!(
        "first reuse wave: {} binding_conflict outcomes; histories={}",
        unbound.iter().filter(|conflict| **conflict).count(),
        json!(runs)
    );

    for (index, (job_id, was_unbound)) in jobs.iter().zip(unbound).enumerate() {
        let first = &runs[index];
        let accepted: Value = client
            .typed("schedule.run_now", json!({"job_id": job_id}))
            .await
            .unwrap();
        let next =
            support::settled(&mut client, job_id, accepted["run_id"].as_str().unwrap()).await;
        assert_eq!(next["state"], "completed", "{next}");
        assert_eq!(next["detail"], "completed", "{next}");
        if was_unbound {
            assert_eq!(next["session_origin"], "launch", "{next}");
            assert_ne!(next["session_id"], first["session_id"], "{next}");
            assert!(expected_sessions.insert(next["session_id"].as_str().unwrap().to_owned()));
        } else {
            assert_eq!(next["session_origin"], "reuse", "{next}");
            assert_eq!(next["session_id"], first["session_id"], "{next}");
        }
        let listed: Value = client.typed("schedule.list", json!({})).await.unwrap();
        let job = listed["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == *job_id)
            .unwrap();
        assert_eq!(job["session_id"], next["session_id"], "{job}");
        assert_eq!(job["revision"], 2, "{job}");
        let history: Value = client
            .typed("schedule.history", json!({"job_id": job_id}))
            .await
            .unwrap();
        assert_eq!(history["runs"].as_array().unwrap().len(), 2, "{history}");
        runs.push(next);
    }
    eprintln!("both reuse waves: histories={}", json!(runs));

    let catalog = client
        .rpc(
            "session.catalog",
            json!({"withInternal": true, "limit": 100}),
        )
        .await
        .unwrap();
    assert!(catalog["nextCursor"].is_null(), "{catalog}");
    let sessions: std::collections::BTreeSet<_> = catalog["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["sessionId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(sessions, expected_sessions, "no extra Sessions launched");

    for session_id in sessions {
        let mut events = Vec::new();
        let mut after = 0;
        let mut through = None;
        loop {
            let mut params = json!({"sessionId": session_id, "branchId": "main", "after": after});
            if let Some(through) = through {
                params["through"] = json!(through);
            }
            let page: dim_client::wire::History =
                client.typed("session.history", params).await.unwrap();
            through = Some(page.through);
            let next = page.next(after, through).unwrap();
            events.extend(page.events);
            match next {
                Some(next) => after = next,
                None => break,
            }
        }
        let inputs: Vec<_> = events
            .iter()
            .filter(|event| {
                event["type"] == "input"
                    && event["name"] == "message"
                    && event["data"]["submission_id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("scheduler:"))
            })
            .collect();
        let session_runs: Vec<_> = runs
            .iter()
            .filter(|run| run["session_id"] == session_id)
            .collect();
        assert_eq!(inputs.len(), session_runs.len(), "{events:?}");
        for run in session_runs {
            let matching: Vec<_> = inputs
                .iter()
                .filter(|event| event["data"]["submission_id"] == run["submission_id"])
                .collect();
            assert_eq!(matching.len(), 1, "exactly one submitted input for {run}");
            assert_eq!(matching[0]["id"], run["input_event_id"], "{run}");
        }
    }
    assert_eq!(model.count(), 16, "one model turn per scheduled input");

    drop(client);
    server.shutdown();
    server.join().await.unwrap();
}

async fn settled(client: &mut Client, job_id: &str, run_id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let history: Value = client
                .typed("schedule.history", json!({"job_id": job_id}))
                .await
                .unwrap();
            let found = history["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|run| run["run_id"] == run_id)
                .cloned();
            if let Some(run) = found
                && !matches!(
                    run["state"].as_str(),
                    Some("launching" | "submitted" | "running")
                )
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the run settles")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reuse_job_binds_its_first_session_and_later_runs_reuse_it() {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir_all(home.path().join("scheduler")).unwrap();

    let model = Arc::new(Script::default());
    let mut builder = AgentBuilder::new(AgentConfig::default());
    defaults::install(&mut builder);
    defaults::mock_hosts_with_model(&mut builder, HistoryRenderer::default(), model.clone());
    install(&mut builder);
    let server = serve(
        builder,
        ServerConfig {
            startup_timeout: Duration::from_secs(60),
            database_path: Some(home.path().join("dimcode.sqlite")),
            product_home: Some(home.path().to_owned()),
            unix_path: Some(home.path().join("native.sock")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let config = ConnectionConfig {
        endpoint: server.unix_path.clone().unwrap(),
        server: Server {
            program: std::env::current_exe().unwrap(),
            embedded: false,
        },
        profile: None,
        client_kind: "cli",
        client_name: "scheduler-reuse-test",
    };
    let mut client = Client::connect(&config).await.unwrap();
    select_model(&mut client).await;

    let saved: Value = client
        .typed(
            "schedule.save",
            json!({"job": {"name": "inbox", "schedule": {"at": "2030-01-01T00:00:00Z"},
                "prompt": "Check the inbox.", "cwd": home.path(), "mode": "reuse"}}),
        )
        .await
        .unwrap();
    let job_id = saved["job"]["id"].as_str().unwrap().to_owned();
    assert!(saved["job"]["session_id"].is_null(), "{saved}");

    let first: Value = client
        .typed("schedule.run_now", json!({"job_id": job_id}))
        .await
        .unwrap();
    let first = settled(&mut client, &job_id, first["run_id"].as_str().unwrap()).await;
    assert_eq!(first["state"], "completed", "{first}");
    assert_eq!(first["session_origin"], "launch", "{first}");
    let bound = first["session_id"].as_str().unwrap().to_owned();

    let listed: Value = client.typed("schedule.list", json!({})).await.unwrap();
    assert_eq!(listed["jobs"][0]["session_id"], json!(bound), "{listed}");
    let diagnostics = server.diagnostics();
    let scheduler = diagnostics
        .plugins
        .iter()
        .find(|plugin| plugin.name == "Scheduler")
        .unwrap();
    assert!(
        scheduler.injects.iter().any(|dependency| dependency.key
            == rutis_bridge::session::host_key("sessions").erase()
            && dependency.status == rutis::host::diagnostics::DependencyStatus::Ready),
        "Scheduler's sessions dependency must be resolved in the composed runtime"
    );

    server.restart("Agents").await.unwrap();

    let second: Value = client
        .typed("schedule.run_now", json!({"job_id": job_id}))
        .await
        .unwrap();
    let second = settled(&mut client, &job_id, second["run_id"].as_str().unwrap()).await;
    assert_eq!(second["state"], "completed", "{second}");
    assert_eq!(second["session_origin"], "reuse", "{second}");
    assert_eq!(second["session_id"], json!(bound), "{second}");

    drop(client);
    server.shutdown();
    server.join().await.unwrap();
}
