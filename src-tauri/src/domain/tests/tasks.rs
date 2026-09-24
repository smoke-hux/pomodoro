use super::*;

#[test]
fn task_and_interruption_crud_maintain_references() {
    let mut data = AppData::default();
    let task = data.create_task("  Read paper  ", 0, 7).unwrap();
    assert_eq!(task.title, "Read paper");
    assert_eq!(task.estimate, 1);
    assert!(data.select_task(Some(task.id.clone())));

    let interruption = data
        .capture_interruption("  Check email  ", InterruptionCategory::External, 8)
        .unwrap();
    assert_eq!(interruption.task_id.as_deref(), Some(task.id.as_str()));
    assert!(data.handle_interruption(&interruption.id));
    assert!(data.interruptions[0].handled);

    assert!(data.delete_task(&task.id));
    assert_eq!(data.timer.active_task_id, None);
    assert_eq!(data.interruptions[0].task_id, None);
    assert!(data.delete_interruption(&interruption.id));
    assert!(data.interruptions.is_empty());
}

#[test]
fn turning_an_interruption_into_a_task_twice_makes_one_task() {
    let mut data = AppData::default();
    let captured = data
        .capture_interruption("Check the deploy", InterruptionCategory::Internal, 10)
        .unwrap();

    let first = data
        .convert_interruption_to_task(&captured.id, 20)
        .expect("the first conversion creates a task");
    let second = data
        .convert_interruption_to_task(&captured.id, 30)
        .expect("the second returns the task the first made");

    assert_eq!(first, second);
    assert_eq!(data.tasks.len(), 1);
    assert_eq!(
        data.interruptions[0].converted_task_id.as_deref(),
        Some(first.as_str())
    );
    // Captured with no task active, and converting does not invent one.
    assert_eq!(data.interruptions[0].task_id, None);
    assert!(data.interruptions[0].handled);

    assert!(data
        .convert_interruption_to_task("interruption-missing", 40)
        .is_none());
}

/// The normal case, and the one that did nothing: a note captured while a
/// task is active already carries that task's id.
#[test]
fn an_interruption_captured_during_a_task_still_becomes_its_own_task() {
    let mut data = AppData::default();
    running_focus(&mut data);
    let active = data.tasks[0].id.clone();
    let captured = data
        .capture_interruption("Renew the certificate", InterruptionCategory::Internal, 10)
        .unwrap();
    assert_eq!(captured.task_id.as_deref(), Some(active.as_str()));
    assert_eq!(captured.converted_task_id, None);

    let first = data
        .convert_interruption_to_task(&captured.id, 20)
        .expect("the conversion creates a task");
    assert_ne!(first, active, "the active task is not the conversion");
    assert_eq!(data.tasks.len(), 2);
    assert_eq!(data.tasks[1].id, first);
    assert_eq!(data.tasks[1].title, "Renew the certificate");
    assert!(data.interruptions[0].handled);
    // Where it was captured is kept; what it became is recorded beside it.
    assert_eq!(
        data.interruptions[0].task_id.as_deref(),
        Some(active.as_str())
    );
    assert_eq!(
        data.interruptions[0].converted_task_id.as_deref(),
        Some(first.as_str())
    );

    let second = data
        .convert_interruption_to_task(&captured.id, 30)
        .expect("the second returns the task the first made");
    assert_eq!(second, first);
    assert_eq!(data.tasks.len(), 2);

    // Deleting the task it became releases the marker, and only that one.
    assert!(data.delete_task(&first));
    assert_eq!(data.interruptions[0].converted_task_id, None);
    assert_eq!(
        data.interruptions[0].task_id.as_deref(),
        Some(active.as_str())
    );
    let third = data
        .convert_interruption_to_task(&captured.id, 40)
        .expect("a released interruption converts again");
    assert_eq!(data.tasks.len(), 2);
    assert_eq!(data.tasks[1].id, third);
}

#[test]
fn an_interruption_stored_before_the_conversion_marker_still_loads() {
    let stored = r#"{
        "id": "interruption-1",
        "text": "Phone call",
        "category": "external",
        "capturedAt": 200,
        "handled": false,
        "taskId": "task-1"
    }"#;
    let interruption: Interruption =
        serde_json::from_str(stored).expect("an older interruption must still load");
    assert_eq!(interruption.task_id.as_deref(), Some("task-1"));
    assert_eq!(interruption.converted_task_id, None);

    // And the field goes out under the name the window reads.
    let value = serde_json::to_value(Interruption {
        converted_task_id: Some("task-2".to_string()),
        ..interruption
    })
    .unwrap();
    assert_eq!(value["convertedTaskId"], "task-2");
    assert_eq!(value["taskId"], "task-1");
}

#[test]
fn same_millisecond_task_burst_keeps_two_thousand_ids_unique() {
    let mut data = AppData::default();
    let mut ids = HashSet::new();

    for index in 0..2_000 {
        let task = data
            .create_task(format!("Queued task {index}"), 1, 42)
            .expect("every non-empty task is accepted");
        assert!(ids.insert(task.id), "a burst reused a task id");
    }

    assert_eq!(data.tasks.len(), 2_000);
    assert_eq!(ids.len(), 2_000);
}
