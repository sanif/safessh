use proptest::prelude::*;
use safessh_audit::query::{query, Filters};
use safessh_audit::sqlite::Index;
use safessh_storage::paths::Paths;
use std::collections::BTreeSet;
use std::io::Write;

fn paths_in(dir: &std::path::Path) -> Paths {
    Paths {
        config: dir.join("config"),
        state: dir.join("state"),
        cache: dir.join("cache"),
    }
}

#[derive(Clone, Debug)]
struct Ev {
    ts: String,
    event_type: String,
    project: String,
    target: Option<String>,
    decision: Option<String>,
    exit_code: Option<i64>,
}

fn ev_to_line(e: &Ev) -> String {
    let mut data = serde_json::Map::new();
    if let Some(t) = &e.target {
        data.insert("target".into(), serde_json::Value::String(t.clone()));
    }
    if let Some(d) = &e.decision {
        data.insert("decision".into(), serde_json::Value::String(d.clone()));
    }
    if let Some(c) = e.exit_code {
        data.insert("exit_code".into(), serde_json::Value::Number(c.into()));
    }
    serde_json::json!({
        "schema_version": 1,
        "timestamp": e.ts,
        "event_type": e.event_type,
        "project": e.project,
        "data": data,
    })
    .to_string()
}

fn ev_strategy() -> impl Strategy<Value = Ev> {
    let day = (1u32..=10u32).prop_map(|d| format!("2026-05-{d:02}T00:00:00Z"));
    let etype = prop_oneof![
        Just("exec_attempt".to_string()),
        Just("exec_complete".to_string())
    ];
    let project = prop_oneof![Just("prod".to_string()), Just("dev".to_string())];
    let target = prop::option::of(prop_oneof![Just("web".to_string()), Just("db".to_string())]);
    let decision = prop::option::of(prop_oneof![
        Just("allow".to_string()),
        Just("deny".to_string()),
        Just("require_approval".to_string())
    ]);
    let exit_code = prop::option::of(0i64..=2);
    (day, etype, project, target, decision, exit_code).prop_map(|(ts, et, p, tgt, d, c)| Ev {
        ts,
        event_type: et,
        project: p,
        target: tgt,
        decision: d,
        exit_code: c,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn query_equals_brute_force(events in prop::collection::vec(ev_strategy(), 1..30)) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(&paths.state).unwrap();
        let log = paths.audit_log();
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .unwrap();
            for e in &events {
                writeln!(f, "{}", ev_to_line(e)).unwrap();
            }
        }
        let mut idx = Index::open_or_create(&paths).unwrap();
        let project = events[0].project.clone();
        let f = Filters {
            project: Some(project.clone()),
            limit: 0,
            ..Filters::default()
        };
        let rows = query(&mut idx, &f).unwrap();
        let got: BTreeSet<String> = rows.into_iter().map(|r| r.raw_json).collect();
        let want: BTreeSet<String> = events.iter()
            .filter(|e| e.project == project)
            .map(ev_to_line)
            .collect();
        prop_assert_eq!(got, want);
    }
}
