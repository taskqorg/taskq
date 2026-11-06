    use super::*;
    // A3/A4, R9/R13: subscription fallback must include Antigravity without overriding pins or unknown ownership.
    #[test]
    fn antigravity_fallback_keeps_pins_and_all_blockers() {
        for reviewer in [false, true] {
            for default in ["claude", "codex", "antigravity"] {
                let (rt, why) = select(&[], reviewer, default, |rt| if rt == "antigravity" { Ok(rt.into()) } else { Err(("usage-limit", format!("{rt} quota"))) }).unwrap();
                assert_eq!(rt, "antigravity");
                assert_eq!(why.is_none(), default == "antigravity");
                for pin in ["claude", "codex", "antigravity", "codex-app"] {
                    assert_eq!(candidates(&[format!("run-{pin}")], reviewer, default), [pin]);
                }
            }
            let error = select(&[], reviewer, "claude", |rt| Err(("capacity", format!("{rt} disabled")))).unwrap_err();
            for rt in ["claude", "codex", "antigravity"] { assert!(error.1.contains(rt), "{error:?}"); }
            let mut tried = vec![];
            assert!(select(&[], reviewer, "claude", |rt| { tried.push(rt.to_string()); Err(("unknown", "host policy unreadable".into())) }).is_err());
            assert_eq!(tried, ["claude"], "unknown common state must not permit fallback");
        }
        assert_eq!(candidates(&["prefer-codex".into()], false, "antigravity"), ["codex", "claude", "antigravity"], "§1 soft worker preference");
        assert!(definition("run-antigravity").is_some());
    }

    // #2137 A2: type/label routing is ordered and soft; pins and explicit preferences win.
    #[test]
    fn balancing_task_type_rules() {
        let rules = routing(&serde_json::json!({"routing":[{"labels":["code","3d"],"prefer":"antigravity"},{"labels":["ui"],"prefer":"codex"}]})).unwrap();
        let labels = vec!["code".into(),"3d".into(),"ui".into()];
        assert_eq!(project_default(&labels,false,&rules,"claude"),"antigravity");
        assert_eq!(project_default(&labels,true,&rules,"claude"),"claude");
        assert_eq!(project_default(&["code".into()],false,&rules,"claude"),"claude");
        let default = project_default(&labels,false,&rules,"claude");
        assert_eq!(route(&["prefer-codex".into()],false,default),"codex");
        assert_eq!(route(&["run-claude".into()],false,default),"claude");
        for rule in [serde_json::json!({"routing":null}),serde_json::json!({"routing":[{"labels":[],"prefer":"codex"}]}),serde_json::json!({"routing":[{"labels":["ui"],"prefer":"codex-app"}]}),serde_json::json!({"routing":[{"labels":[42],"prefer":"codex"}]})] { assert!(routing(&rule).is_err(),"{rule}"); }
    }

    // #2137 A2: soft preferences must fall back; explicit pins must still bind both roles.
    #[test]
    fn balancing_soft_preferences_fall_back() {
        for rt in ["claude", "codex", "antigravity"] {
            let label = vec![format!("prefer-{rt}")];
            assert_eq!(route(&label, false, "claude"), rt);
            assert_eq!(candidates(&label, false, "claude").len(), 3);
            let (chosen, reason) = select(&label, false, "claude", |c| if c == rt { Err(("capacity", format!("{c} disabled (limit 0)"))) } else { Ok(c.into()) }).unwrap();
            assert_ne!(chosen, rt);
            assert!(reason.unwrap().contains("disabled (limit 0)"));
            assert_eq!(candidates(&[format!("run-{rt}")], true, "codex"), [rt]);
        }
        assert_eq!(route(&["prefer-claude".into(), "prefer-codex".into()],false,"antigravity"),"antigravity","conflicting board preferences remain ignored");
    }

    // Chain: § 1 task labels on add. Risk: conflicting priority, runtime or host labels accepted.
    #[test]
    fn catalog_and_aliases() {
        for n in FIXED { let (hex, desc) = definition(n).unwrap(); assert_eq!(hex.len(), 6); assert!(desc.contains(';')); }
        assert!(definition("run-any").is_none());
        assert!(definition("bug").is_none());
        assert!(matches(&["priority-1".into()], "p1"));
        assert!(matches(&["q-blocked".into()], "q-waiting"));
        assert!(validate(&["p1".into(), "priority-1".into()]).is_ok());
        for pair in [["p1", "priority-2"], ["run-codex", "run-claude"], ["host-win", "host-mac"], ["code", "docs"]] {
            assert!(validate(&pair.map(String::from)).is_err());
        }
        assert!(validate(&["needs-a".into(), "needs-b".into()]).is_ok());
    }

    // Chain: § 4 Filter -> host.json `default_runtime` -> admitted runtime. Risk: a host without claude
    // routes its reviews or unlabelled work to claude and waits forever, or the default overrides `run-*`.
    #[test]
    fn route_uses_the_host_default_runtime_unless_run_names_one() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for default in crate::runtime::CLI_RUNTIMES {
            assert_eq!(route(&l(&["p1"]), true, default), default, "reviewer");
            assert_eq!(route(&l(&["run-claude"]), true, default), "claude", "explicit pin binds review");
            assert_eq!(route(&l(&[]), false, default), default, "worker without run-*");
            assert_eq!(route(&l(&["prefer-claude"]), false, default), "claude");
            assert_eq!(route(&l(&["prefer-codex", "prefer-claude"]), false, default), default);
            assert_eq!(route(&l(&["prefer-codex"]), false, default), "codex");
            assert_eq!(route(&l(&["run-claude"]), false, default), "claude", "run-* wins");
            assert_eq!(route(&l(&["run-codex-app", "prefer-codex"]), false, default), "codex-app");
        }
    }
