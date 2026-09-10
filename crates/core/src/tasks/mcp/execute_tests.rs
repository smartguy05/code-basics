use super::*;

fn seeded() -> TasksFile {
    let mut file = TasksFile::default();
    file.create("t1", "First", "first body", TaskOwner::Me, 100);
    file.create("t2", "Second", "second body", TaskOwner::Me, 100);
    file
}

#[test]
fn list_returns_every_task_and_does_not_mutate() {
    let mut file = seeded();
    let outcome = apply(&mut file, ToolCall::ListTasks, 200, "new").unwrap();
    match &outcome {
        Outcome::List(tasks) => assert_eq!(tasks.len(), 2),
        other => panic!("{other:?}"),
    }
    assert!(!outcome.mutated());
}

#[test]
fn get_returns_the_named_task() {
    let mut file = seeded();
    let outcome = apply(&mut file, ToolCall::GetTask { id: "t2".into() }, 200, "new").unwrap();
    match outcome {
        Outcome::One(task) => assert_eq!(task.title, "Second"),
        other => panic!("{other:?}"),
    }
    assert!(
        !apply(&mut file, ToolCall::GetTask { id: "t2".into() }, 200, "new")
            .unwrap()
            .mutated()
    );
}

#[test]
fn get_of_a_missing_id_is_not_found() {
    let mut file = seeded();
    let err = apply(
        &mut file,
        ToolCall::GetTask { id: "nope".into() },
        200,
        "new",
    )
    .unwrap_err();
    assert_eq!(err, McpRefusal::TaskNotFound { id: "nope".into() });
}

#[test]
fn create_appends_an_open_me_task_with_the_injected_id_and_clock() {
    let mut file = TasksFile::default();
    let outcome = apply(
        &mut file,
        ToolCall::CreateTask {
            title: "New".into(),
            body: Some("desc".into()),
        },
        555,
        "generated-id",
    )
    .unwrap();
    match outcome {
        Outcome::Created(task) => {
            assert_eq!(task.id, "generated-id");
            assert_eq!(task.title, "New");
            assert_eq!(task.body, "desc");
            assert_eq!(task.status, TaskStatus::Open);
            assert_eq!(task.owner, TaskOwner::Me);
            assert_eq!(task.created_at_ms, 555);
            assert_eq!(task.updated_at_ms, 555);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(file.tasks.len(), 1);
}

#[test]
fn create_without_a_body_gets_an_empty_one() {
    let mut file = TasksFile::default();
    let outcome = apply(
        &mut file,
        ToolCall::CreateTask {
            title: "Titleonly".into(),
            body: None,
        },
        1,
        "id",
    )
    .unwrap();
    match outcome {
        Outcome::Created(task) => assert_eq!(task.body, ""),
        other => panic!("{other:?}"),
    }
}

#[test]
fn update_omitting_a_field_keeps_it_and_present_fields_replace() {
    let mut file = seeded();
    let outcome = apply(
        &mut file,
        ToolCall::UpdateTask {
            id: "t1".into(),
            title: Some("Renamed".into()),
            body: None,
        },
        900,
        "new",
    )
    .unwrap();
    assert!(outcome.mutated());
    match outcome {
        Outcome::Updated(task) => {
            assert_eq!(task.title, "Renamed");
            assert_eq!(task.body, "first body", "omitted body is unchanged");
            assert_eq!(task.created_at_ms, 100, "creation time preserved");
            assert_eq!(task.updated_at_ms, 900);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn update_can_clear_a_field_with_an_empty_string() {
    let mut file = seeded();
    apply(
        &mut file,
        ToolCall::UpdateTask {
            id: "t1".into(),
            title: None,
            body: Some(String::new()),
        },
        900,
        "new",
    )
    .unwrap();
    assert_eq!(file.tasks[0].body, "");
    assert_eq!(file.tasks[0].title, "First", "omitted title unchanged");
}

#[test]
fn update_of_a_missing_id_is_not_found() {
    let mut file = seeded();
    let err = apply(
        &mut file,
        ToolCall::UpdateTask {
            id: "nope".into(),
            title: Some("x".into()),
            body: None,
        },
        900,
        "new",
    )
    .unwrap_err();
    assert_eq!(err, McpRefusal::TaskNotFound { id: "nope".into() });
}

#[test]
fn complete_marks_done_and_stamps_updated() {
    let mut file = seeded();
    let outcome = apply(
        &mut file,
        ToolCall::CompleteTask { id: "t1".into() },
        1234,
        "new",
    )
    .unwrap();
    assert!(outcome.mutated());
    match outcome {
        Outcome::Completed(task) => {
            assert_eq!(task.status, TaskStatus::Done);
            assert_eq!(task.updated_at_ms, 1234);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn delete_removes_the_task_and_reports_its_id() {
    let mut file = seeded();
    let outcome = apply(
        &mut file,
        ToolCall::DeleteTask { id: "t1".into() },
        200,
        "new",
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Deleted { id: "t1".into() });
    assert!(outcome.mutated());
    assert_eq!(file.tasks.len(), 1);
    assert_eq!(file.tasks[0].id, "t2");
}

#[test]
fn delete_of_a_missing_id_is_not_found() {
    let mut file = seeded();
    let err = apply(
        &mut file,
        ToolCall::DeleteTask { id: "nope".into() },
        200,
        "new",
    )
    .unwrap_err();
    assert_eq!(err, McpRefusal::TaskNotFound { id: "nope".into() });
}

#[test]
fn assign_sets_the_owner_and_stamps_updated() {
    let mut file = seeded();
    let outcome = apply(
        &mut file,
        ToolCall::AssignTask {
            id: "t1".into(),
            owner: TaskOwner::Ai,
        },
        777,
        "new",
    )
    .unwrap();
    match outcome {
        Outcome::Assigned(task) => {
            assert_eq!(task.owner, TaskOwner::Ai);
            assert_eq!(task.updated_at_ms, 777);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn assign_of_a_missing_id_is_not_found() {
    let mut file = seeded();
    let err = apply(
        &mut file,
        ToolCall::AssignTask {
            id: "nope".into(),
            owner: TaskOwner::Ai,
        },
        777,
        "new",
    )
    .unwrap_err();
    assert_eq!(err, McpRefusal::TaskNotFound { id: "nope".into() });
}

#[test]
fn every_write_outcome_reports_mutated_and_every_read_does_not() {
    let created = Outcome::Created(sample());
    let updated = Outcome::Updated(sample());
    let completed = Outcome::Completed(sample());
    let assigned = Outcome::Assigned(sample());
    let deleted = Outcome::Deleted { id: "t1".into() };
    for o in [created, updated, completed, assigned, deleted] {
        assert!(o.mutated(), "{o:?} should be a write");
    }
    assert!(!Outcome::List(vec![]).mutated());
    assert!(!Outcome::One(sample()).mutated());
}

fn sample() -> Task {
    Task {
        id: "t1".into(),
        title: "t".into(),
        body: "b".into(),
        status: TaskStatus::Open,
        owner: TaskOwner::Me,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}
