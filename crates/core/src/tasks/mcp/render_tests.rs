use super::*;

fn task(id: &str, title: &str, status: TaskStatus, owner: TaskOwner, body: &str) -> Task {
    Task {
        id: id.into(),
        title: title.into(),
        body: body.into(),
        status,
        owner,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

#[test]
fn an_empty_list_is_stated_as_complete_not_missing() {
    let text = list(&[]);
    assert!(text.contains("no tasks"));
    assert!(text.contains("complete answer"));
}

#[test]
fn a_line_carries_id_status_owner_and_title() {
    let text = line(&task(
        "t1",
        "Do it",
        TaskStatus::Open,
        TaskOwner::Ai,
        "body",
    ));
    assert!(text.contains("t1"));
    assert!(text.contains("open"));
    assert!(text.contains("owner:ai"));
    assert!(text.contains("Do it"));
}

#[test]
fn a_done_task_reads_done() {
    let text = line(&task("t1", "x", TaskStatus::Done, TaskOwner::Me, ""));
    assert!(text.contains("done"));
    assert!(text.contains("owner:me"));
}

#[test]
fn detail_includes_the_body_when_there_is_one() {
    let text = detail(&task(
        "t1",
        "Title",
        TaskStatus::Open,
        TaskOwner::Me,
        "the body",
    ));
    assert!(text.contains("Title"));
    assert!(text.contains("the body"));
}

#[test]
fn detail_omits_an_empty_body() {
    let text = detail(&task("t1", "Title", TaskStatus::Open, TaskOwner::Me, ""));
    assert_eq!(text.lines().count(), 1);
}

#[test]
fn each_outcome_names_the_verb_it_performed() {
    let t = task("t1", "Title", TaskStatus::Open, TaskOwner::Me, "");
    assert!(outcome(&Outcome::Created(t.clone())).contains("Created"));
    assert!(outcome(&Outcome::Updated(t.clone())).contains("Updated"));
    assert!(outcome(&Outcome::Completed(t.clone())).contains("done"));
    assert!(outcome(&Outcome::Assigned(task(
        "t1",
        "T",
        TaskStatus::Open,
        TaskOwner::Ai,
        ""
    )))
    .contains("ai"));
    assert!(outcome(&Outcome::Deleted { id: "t1".into() }).contains("Deleted task t1"));
}

#[test]
fn a_list_outcome_renders_every_task() {
    let text = outcome(&Outcome::List(vec![
        task("t1", "One", TaskStatus::Open, TaskOwner::Me, ""),
        task("t2", "Two", TaskStatus::Done, TaskOwner::Ai, ""),
    ]));
    assert!(text.contains("t1"));
    assert!(text.contains("t2"));
    assert_eq!(text.lines().count(), 2);
}
