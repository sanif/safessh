//! Insta snapshots for the Projects screen — empty state and a populated
//! project with two targets + non-trivial policy.

use ratatui::{backend::TestBackend, Terminal};
use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};
use safessh_tui::screens::projects::ProjectsScreen;

fn paths() -> Paths {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    std::mem::forget(tmp);
    p
}

fn render(screen: &ProjectsScreen) -> String {
    let backend = TestBackend::new(80, 20);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| screen.render(f, f.area())).unwrap();
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let pos = ratatui::layout::Position::new(x, y);
            out.push_str(buf.cell(pos).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    out
}

#[test]
fn empty_state() {
    let p = paths();
    let screen = ProjectsScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&screen));
}

#[test]
fn one_project_with_two_targets() {
    let p = paths();
    let store = ProjectStore::new(p.clone());
    store
        .save(&Project {
            name: "prod".into(),
            default_target: "web".into(),
            targets: vec![
                Target::SshConfigAlias {
                    name: "web".into(),
                    ssh_config_alias: "prod-web".into(),
                },
                Target::Inline {
                    name: "db".into(),
                    host: "10.0.0.5".into(),
                    port: 22,
                    user: "deploy".into(),
                    identity_file: None,
                    proxy_jump: None,
                    keychain_secret: None,
                },
            ],
            policy: Policy {
                allow: vec!["read:safe".into()],
                require_approval: vec!["destructive:filesystem".into()],
                deny: vec!["destructive:disk".into()],
            },
            approvals: Approvals::default(),
            output: OutputCaps::default(),
        })
        .unwrap();
    let screen = ProjectsScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&screen));
}
