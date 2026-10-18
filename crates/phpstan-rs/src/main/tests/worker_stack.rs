//! The thread the command runs on.

use super::super::on_worker_stack;
use phpstan_rs::project::WORKER_STACK_SIZE;

#[test]
fn the_body_runs_on_a_thread_with_the_stack_size_asked_for() {
    let remaining = on_worker_stack(|| {
        stacker::remaining_stack().expect("the system gives the end of the stack")
    });
    // Nearly all of the stack is left at the start of the thread.
    assert!(remaining <= WORKER_STACK_SIZE, "{remaining}");
    assert!(remaining > WORKER_STACK_SIZE - 1024 * 1024, "{remaining}");
}

#[test]
fn the_body_runs_on_another_thread_and_its_result_comes_back() {
    let caller = std::thread::current().id();
    let (ran_on, value) = on_worker_stack(|| (std::thread::current().id(), 7));
    assert_ne!(ran_on, caller);
    assert_eq!(value, 7);
}

#[test]
fn a_panic_of_the_body_is_raised_again_in_the_caller() {
    let outcome = std::panic::catch_unwind(|| on_worker_stack(|| panic!("the body panics")));
    let payload = outcome.expect_err("the panic reaches the caller");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"the body panics"));
}
