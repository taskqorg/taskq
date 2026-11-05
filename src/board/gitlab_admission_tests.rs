    use super::*;
    #[test]
    fn earliest_label_event_is_the_only_arrival() {
        let mut events = json!([{"id":9,"event":"labeled","action":"add","label":{"name":"taskq"},"created_at":"2026-10-30T12:01:00Z"},{"id":3,"event":"labeled","action":"add","label":{"name":"taskq"},"created_at":"2026-10-30T12:00:00Z"},{"id":2,"event":"unlabeled","action":"remove","label":{"name":"taskq"},"created_at":"2026-10-30T11:59:00Z"}]);
        assert_eq!(first_admission(events.as_array().unwrap()).unwrap(), Some((3, iso("2026-10-30T12:00:00Z").unwrap())));
        events[1]["created_at"] = Value::Null;
        assert!(first_admission(events.as_array().unwrap()).is_err());
        assert_eq!(first_admission(&[]).unwrap(), None);
    }

    // #1960 defect: a deleted label's events (label null, csgo #88) failed the source on every pass.
    // Chain: §4 admission records. Risk kept: a deleted label invents or hides a `taskq` admission.
    #[test]
    fn deleted_label_events_admit_nothing() {
        let deleted = json!([{"id":5,"action":"add","label":null,"created_at":"2026-10-05T10:00:00Z"},{"id":6,"action":"remove","label":null,"created_at":"2026-10-05T11:00:00Z"}]);
        assert_eq!(first_admission(deleted.as_array().unwrap()).unwrap(), None);
        let mut events = deleted.as_array().unwrap().clone();
        events.push(json!({"id":7,"action":"add","label":{"name":"taskq"},"created_at":"2026-10-06T10:00:00Z"}));
        assert_eq!(first_admission(&events).unwrap(), Some((7, iso("2026-10-06T10:00:00Z").unwrap())));
    }
