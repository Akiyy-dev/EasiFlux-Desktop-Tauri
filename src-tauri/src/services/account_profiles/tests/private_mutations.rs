use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::super::{
    run_account_private_mutation, run_account_private_operation, run_account_public_operation,
    AccountLifecycleCoordinator,
};

#[tokio::test]
async fn private_read_side_effects_finish_before_a_later_account_switch() {
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let events = Arc::new(Mutex::new(Vec::new()));
    let api_started = Arc::new(tokio::sync::Barrier::new(3));
    let release_api = Arc::new(tokio::sync::Notify::new());

    let read_coordinator = Arc::clone(&coordinator);
    let read_events = Arc::clone(&events);
    let read_started = Arc::clone(&api_started);
    let read_release = Arc::clone(&release_api);
    let private_read = async move {
        run_account_private_operation(read_coordinator.as_ref(), || async move {
            read_events.lock().unwrap().push("read:context");
            read_started.wait().await;
            read_release.notified().await;
            read_events.lock().unwrap().push("read:http");
            read_events.lock().unwrap().push("read:map");
            read_events.lock().unwrap().push("read:emit");
            read_events.lock().unwrap().push("read:analytics");
        })
        .await;
    };

    let switch_coordinator = Arc::clone(&coordinator);
    let switch_events = Arc::clone(&events);
    let switch_started = Arc::clone(&api_started);
    let account_switch = async move {
        switch_started.wait().await;
        let _guard = switch_coordinator.mutation_guard().await;
        switch_events.lock().unwrap().push("switch");
    };

    let observer_events = Arc::clone(&events);
    let observer_started = Arc::clone(&api_started);
    let observer = async move {
        observer_started.wait().await;
        tokio::task::yield_now().await;
        assert_eq!(*observer_events.lock().unwrap(), ["read:context"]);
        release_api.notify_one();
    };

    let ((), (), ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(private_read, account_switch, observer)
    })
    .await
    .expect("private read and switch should not deadlock");
    assert_eq!(
        *events.lock().unwrap(),
        [
            "read:context",
            "read:http",
            "read:map",
            "read:emit",
            "read:analytics",
            "switch",
        ]
    );
}

#[tokio::test]
async fn private_mutation_body_waits_for_the_lifecycle_guard() {
    let coordinator = AccountLifecycleCoordinator::new();
    let held_guard = coordinator.mutation_guard().await;
    let body_ran = AtomicBool::new(false);

    let mutation = run_account_private_mutation(&coordinator, || async {
        body_ran.store(true, Ordering::SeqCst);
    });
    tokio::pin!(mutation);

    assert!(matches!(
        futures_util::poll!(&mut mutation),
        std::task::Poll::Pending
    ));
    assert!(!body_ran.load(Ordering::SeqCst));

    drop(held_guard);
    tokio::time::timeout(std::time::Duration::from_secs(1), mutation.as_mut())
        .await
        .expect("private mutation should run after account switching releases the guard");
    assert!(body_ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn in_flight_private_mutation_finishes_before_a_later_switch() {
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let events = Arc::new(Mutex::new(Vec::new()));
    let started = Arc::new(tokio::sync::Barrier::new(3));
    let release = Arc::new(tokio::sync::Notify::new());

    let mutation_coordinator = Arc::clone(&coordinator);
    let mutation_events = Arc::clone(&events);
    let mutation_started = Arc::clone(&started);
    let mutation_release = Arc::clone(&release);
    let mutation = async move {
        run_account_private_mutation(mutation_coordinator.as_ref(), || async move {
            mutation_events.lock().unwrap().push("mutation:start");
            mutation_started.wait().await;
            mutation_release.notified().await;
            mutation_events.lock().unwrap().push("mutation:end");
        })
        .await;
    };

    let switch_coordinator = Arc::clone(&coordinator);
    let switch_events = Arc::clone(&events);
    let switch_started = Arc::clone(&started);
    let account_switch = async move {
        switch_started.wait().await;
        let _guard = switch_coordinator.mutation_guard().await;
        switch_events.lock().unwrap().push("switch");
    };

    let observed_events = Arc::clone(&events);
    let observer_started = Arc::clone(&started);
    let observer_release = Arc::clone(&release);
    let observer = async move {
        observer_started.wait().await;
        assert_eq!(*observed_events.lock().unwrap(), ["mutation:start"]);
        observer_release.notify_one();
    };

    let ((), (), ()) = tokio::join!(mutation, account_switch, observer);
    assert_eq!(
        *events.lock().unwrap(),
        ["mutation:start", "mutation:end", "switch"]
    );
}

#[tokio::test]
async fn shared_public_request_waits_for_a_switch_and_uses_the_committed_environment() {
    let coordinator = AccountLifecycleCoordinator::new();
    let environment = Mutex::new("old");
    let cache_and_time = Mutex::new(None::<String>);
    let held_switch = coordinator.mutation_guard().await;

    let request = run_account_public_operation(&coordinator, || async {
        let selected = *environment.lock().unwrap();
        *cache_and_time.lock().unwrap() = Some(format!("{selected}:cache+time"));
    });
    tokio::pin!(request);

    assert!(matches!(
        futures_util::poll!(&mut request),
        std::task::Poll::Pending
    ));
    assert!(cache_and_time.lock().unwrap().is_none());

    *environment.lock().unwrap() = "new";
    drop(held_switch);
    tokio::time::timeout(std::time::Duration::from_secs(1), request.as_mut())
        .await
        .expect("public request should run after the switch releases the guard");

    assert_eq!(
        cache_and_time.lock().unwrap().as_deref(),
        Some("new:cache+time")
    );
}

#[tokio::test]
async fn in_flight_public_request_and_commit_finish_before_switch_clears_old_state() {
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let environment = Arc::new(Mutex::new("old"));
    let cache_and_time = Arc::new(Mutex::new(None::<String>));
    let events = Arc::new(Mutex::new(Vec::new()));
    let started = Arc::new(tokio::sync::Barrier::new(3));
    let release = Arc::new(tokio::sync::Notify::new());

    let request_coordinator = Arc::clone(&coordinator);
    let request_environment = Arc::clone(&environment);
    let request_state = Arc::clone(&cache_and_time);
    let request_events = Arc::clone(&events);
    let request_started = Arc::clone(&started);
    let request_release = Arc::clone(&release);
    let request = async move {
        run_account_public_operation(request_coordinator.as_ref(), || async move {
            let selected = *request_environment.lock().unwrap();
            request_events
                .lock()
                .unwrap()
                .push(format!("request:{selected}"));
            request_started.wait().await;
            request_release.notified().await;
            *request_state.lock().unwrap() = Some(format!("{selected}:cache+time"));
            request_events
                .lock()
                .unwrap()
                .push(format!("commit:{selected}"));
        })
        .await;
    };

    let switch_coordinator = Arc::clone(&coordinator);
    let switch_environment = Arc::clone(&environment);
    let switch_state = Arc::clone(&cache_and_time);
    let switch_events = Arc::clone(&events);
    let switch_started = Arc::clone(&started);
    let account_switch = async move {
        switch_started.wait().await;
        let _guard = switch_coordinator.mutation_guard().await;
        *switch_environment.lock().unwrap() = "new";
        *switch_state.lock().unwrap() = None;
        switch_events.lock().unwrap().push("switch:new".into());
    };

    let observer_events = Arc::clone(&events);
    let observer_started = Arc::clone(&started);
    let observer_release = Arc::clone(&release);
    let observer = async move {
        observer_started.wait().await;
        assert_eq!(*observer_events.lock().unwrap(), ["request:old"]);
        observer_release.notify_one();
    };

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(request, account_switch, observer)
    })
    .await
    .expect("public request and switch should not deadlock");

    assert_eq!(
        *events.lock().unwrap(),
        ["request:old", "commit:old", "switch:new"]
    );
    assert_eq!(*environment.lock().unwrap(), "new");
    assert!(cache_and_time.lock().unwrap().is_none());
}

#[tokio::test]
async fn independent_account_reads_can_run_concurrently() {
    let coordinator = AccountLifecycleCoordinator::new();
    let first_started = Arc::new(AtomicBool::new(false));
    let release_first = Arc::new(tokio::sync::Notify::new());

    let started = Arc::clone(&first_started);
    let release = Arc::clone(&release_first);
    let first = run_account_private_operation(&coordinator, move || async move {
        started.store(true, Ordering::SeqCst);
        release.notified().await;
    });
    tokio::pin!(first);
    assert!(matches!(
        futures_util::poll!(&mut first),
        std::task::Poll::Pending
    ));
    assert!(first_started.load(Ordering::SeqCst));

    let second_ran = AtomicBool::new(false);
    let second = run_account_public_operation(&coordinator, || async {
        second_ran.store(true, Ordering::SeqCst);
    });
    tokio::pin!(second);

    assert!(matches!(
        futures_util::poll!(&mut second),
        std::task::Poll::Ready(())
    ));
    assert!(second_ran.load(Ordering::SeqCst));

    release_first.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(1), first.as_mut())
        .await
        .expect("the first read should finish after release");
}

#[tokio::test]
async fn queued_account_mutation_precedes_later_reads() {
    let coordinator = AccountLifecycleCoordinator::new();
    let events = Mutex::new(Vec::new());
    let release_first = tokio::sync::Notify::new();

    let first = run_account_private_operation(&coordinator, || async {
        events.lock().unwrap().push("read:first:start");
        release_first.notified().await;
        events.lock().unwrap().push("read:first:end");
    });
    tokio::pin!(first);
    assert!(matches!(
        futures_util::poll!(&mut first),
        std::task::Poll::Pending
    ));

    let mutation = super::super::run_serialized_account_mutation(&coordinator, || async {
        events.lock().unwrap().push("mutation");
    });
    tokio::pin!(mutation);
    assert!(matches!(
        futures_util::poll!(&mut mutation),
        std::task::Poll::Pending
    ));

    let later_read = run_account_public_operation(&coordinator, || async {
        events.lock().unwrap().push("read:later");
    });
    tokio::pin!(later_read);
    assert!(matches!(
        futures_util::poll!(&mut later_read),
        std::task::Poll::Pending
    ));

    release_first.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(first.as_mut(), mutation.as_mut(), later_read.as_mut())
    })
    .await
    .expect("writer priority should allow both queued operations to finish");

    assert_eq!(
        *events.lock().unwrap(),
        [
            "read:first:start",
            "read:first:end",
            "mutation",
            "read:later"
        ]
    );
}
