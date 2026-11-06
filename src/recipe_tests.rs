use super::*;
use serde_json::json;

fn cfg() -> Value {
    json!({"recipes": {"feature": {"version": 2, "tasks": [
        {"title": "Spec {goal}", "goal": "Decide {goal}", "acceptance": ["spec merged"], "gate": true, "type": "docs"},
        {"title": "Code {goal}", "goal": "Build {goal}", "acceptance": ["tests pass"], "deps": [1], "priority": 1}
    ]}}})
}

// Chain: taskq.json recipe -> load -> held NewTask list. Risk: a template that names a later task, or a run
// without --goal, adds half a recipe before failing.
#[test]
fn load_checks_the_whole_recipe_before_any_write() {
    let (v, items) = load(&cfg(), "feature", "r1", Some("login")).unwrap();
    assert_eq!(v, "2");
    assert_eq!(items[0].task.title, "Spec login");
    assert!(items[0].gate && items[0].task.labels == ["needs-gate"]);
    assert_eq!(
        (items[1].deps.clone(), items[1].task.priority.clone()),
        (vec![1], Some("1".into()))
    );
    assert_eq!(
        load(&cfg(), "feature", "r1", None).unwrap_err(),
        "recipe `feature` needs --goal"
    );
    assert!(
        load(&cfg(), "nope", "r1", None)
            .unwrap_err()
            .starts_with("no recipe")
    );
    assert!(
        load(&cfg(), "feature", "r 1", Some("x"))
            .unwrap_err()
            .starts_with("--run")
    );
    let bad = json!({"recipes": {"x": {"version": "1", "tasks": [{"title": "A", "goal": "G", "acceptance": ["a"], "deps": [1]}]}}});
    assert!(
        load(&bad, "x", "r", None)
            .unwrap_err()
            .contains("not an earlier task")
    );
}
