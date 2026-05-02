use nu_test_support::nu;
use nu_test_support::tester::test;

#[test]
fn try_succeed() {
    let output = nu!("try { 345 } catch { echo 'hello' }");

    assert!(output.out.contains("345"));
}

#[test]
fn try_catch() {
    let output = nu!("try { foobarbaz } catch { echo 'hello' }");

    assert!(output.out.contains("hello"));
}

#[test]
fn catch_can_access_error() {
    let output = nu!("try { foobarbaz } catch { |err| $err | get raw }");

    assert!(output.err.contains("External command failed"));
}

#[test]
fn catch_can_access_error_as_dollar_in() {
    let output = nu!("try { foobarbaz } catch { $in | get raw }");

    assert!(output.err.contains("External command failed"));
}

#[test]
fn external_failed_should_be_caught() {
    let output = nu!("try { nu --testbin fail; echo 'success' } catch { echo 'fail' }");

    assert!(output.out.contains("fail"));
}

#[test]
fn loop_try_break_should_be_successful() {
    let output =
        nu!("loop { try { print 'successful'; break } catch { print 'failed'; continue } }");

    assert_eq!(output.out, "successful");
}

#[test]
fn loop_try_break_should_pop_error_handlers() {
    let output = nu!(r#"
    do {
        loop {
            try {
                break
            } catch {
                print 'jumped to catch block'
                return
            }
        }
        error make -u {msg: "success"}
    }
    "#);

    assert!(!output.status.success(), "error was caught");
    assert!(output.err.contains("success"));
}

#[test]
fn loop_nested_try_break_should_pop_error_handlers() {
    let output = nu!(r#"
    do {
        loop {
            try {
                try {
                    break
                } catch {
                    print 'jumped to inner catch block'
                    return
                }
            } catch {
                print 'jumped to outer catch block'
                return
            }
        }
        error make -u {msg: "success"}
    }
    "#);

    assert!(!output.status.success(), "error was caught");
    assert!(output.err.contains("success"));
}

#[test]
fn loop_try_continue_should_pop_error_handlers() {
    let output = nu!(r#"
    do {
        mut error = false

        loop {
            if $error {
                error make -u {msg: "success"}
            }

            try {
                $error = true
                continue
            } catch {
                print 'jumped to catch block'
                return
            }
        }
    }
    "#);

    assert!(!output.status.success(), "error was caught");
    assert!(output.err.contains("success"));
}

#[test]
fn loop_catch_break_should_show_failed() {
    let output = nu!("loop {
            try { invalid 1;
            continue; } catch { print 'failed'; break }
        }
        ");

    assert_eq!(output.out, "failed");
}

#[test]
fn loop_try_ignores_continue() {
    let output = nu!("mut total = 0;
        for i in 0..10 {
            try { if ($i mod 2) == 0 {
            continue;}
            $total += 1
        } catch { echo 'failed'; break }
        }
        echo $total
        ");

    assert_eq!(output.out, "5");
}

#[test]
fn loop_try_break_on_command_should_show_successful() {
    let output = nu!("loop { try { ls; break } catch { echo 'failed';continue }}");

    assert!(!output.out.contains("failed"));
}

#[test]
fn catch_block_can_use_error_object() {
    let output = nu!("try {1 / 0} catch {|err| print ($err | get msg)}");
    assert_eq!(output.out, "Division by zero.")
}

#[test]
fn catch_input_type_mismatch_and_rethrow() {
    let actual = nu!(
        "let x: any = 1; try { $x | get 1 } catch {|err| error make { msg: ($err | get msg) } }"
    );
    assert!(actual.err.contains("Input type not supported"));
}

// This test is disabled on Windows because they cause a stack overflow in CI (but not locally!).
// For reasons we don't understand, the Windows CI runners are prone to stack overflow.
// TODO: investigate so we can enable on Windows
#[cfg(not(target_os = "windows"))]
#[test]
fn can_catch_infinite_recursion() {
    let actual = nu!(r#"
            def bang [] { try { bang } catch { "Caught infinite recursion" } }; bang
        "#);
    assert_eq!(actual.out, "Caught infinite recursion");
}

#[test]
fn exit_code_available_in_catch_env() {
    let actual = nu!("try { nu -c 'exit 42' } catch { $env.LAST_EXIT_CODE }");
    assert_eq!(actual.out, "42");
}

#[test]
fn exit_code_available_in_catch() {
    let actual = nu!("try { nu -c 'exit 42' } catch { |e| $e.exit_code }");
    assert_eq!(actual.out, "42");
}

#[test]
fn catches_exit_code_in_assignment() {
    let actual = nu!("let x = try { nu -c 'exit 42' } catch { |e| $e.exit_code }; $x");
    assert_eq!(actual.out, "42");
}

#[test]
fn catches_exit_code_in_expr() {
    let actual = nu!("print (try { nu -c 'exit 42' } catch { |e| $e.exit_code })");
    assert_eq!(actual.out, "42");
}

#[test]
fn prints_only_if_last_pipeline() {
    let actual = nu!("try { 'should not print' }; 'last value'");
    assert_eq!(actual.out, "last value");

    let actual = nu!("try { ['should not print'] | every 1 }; 'last value'");
    assert_eq!(actual.out, "last value");
}

#[test]
fn get_error_columns() {
    let actual = nu!(" try { non_existent_command } catch {|err| $err} | columns | to json -r");
    assert_eq!(
        actual.out,
        "[\"msg\",\"debug\",\"raw\",\"rendered\",\"json\"]"
    );
}

#[test]
fn get_json_error() {
    let actual = nu!(
        "try { non_existent_command } catch {|err| $err} | get json | from json | update labels.span {{start: 0 end: 0}} | to json -r"
    );
    assert_eq!(
        actual.out,
        "{\"msg\":\"External command failed\",\"labels\":[{\"text\":\"Command `non_existent_command` not found\",\"span\":{\"start\":0,\"end\":0}}],\"code\":\"nu::shell::external_command\",\"url\":null,\"help\":\"`non_existent_command` is neither a Nushell built-in or a known external command\",\"inner\":[]}"
    );
}

#[test]
fn pipefail_works() {
    // the print 'bbb' should not run because the previous command failed
    // So no output should be printed
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        "nu --testbin fail | lines | length; print 'bbb'"
    );
    assert_eq!(actual.out, "")
}

#[test]
fn let_ignores_pipefail() {
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        "try { let x = nu --testbin fail | lines | length; print $x } catch {|e| print $e.exit_code}"
    );
    assert_eq!(actual.out, "0")
}

#[test]
fn try_catch_finally() {
    // catch should run because try failed, then finally should run.
    let actual =
        nu!("try { 1 / 0 } catch { print 'inside catch' } finally { print 'this finally' }");
    assert!(actual.out.contains("inside catch"));
    assert!(actual.out.contains("this finally"));
    assert!(!actual.err.contains("division by zero"));

    // catch should not run because try success, then finally should run.
    let actual = nu!(
        "try { print 'inside try' } catch { print 'inside catch' } finally { print 'this finally' }"
    );
    assert!(actual.out.contains("inside try"));
    assert!(actual.out.contains("this finally"));
    assert!(!actual.out.contains("inside catch"));

    // catch should run even if error inside catch.
    let actual =
        nu!("try { 1 / 0 } catch { 1 / 0; print 'inside catch' } finally { print 'this finally' }");
    assert!(actual.out.contains("this finally"));
    assert!(!actual.out.contains("inside catch"));
    assert!(actual.err.contains("division by zero"));
}

#[test]
fn try_finally() {
    let actual = nu!("try { 0 } finally { 3 }");
    assert_eq!(actual.out, "0");

    let actual = nu!("try { 1 / 0 } finally { print 'this finally' }");
    assert!(actual.out.contains("this finally"));
    assert!(actual.err.contains("division by zero"));

    let actual = nu!("try { print 'inside try' } finally { print 'this finally' }");
    assert!(actual.out.contains("inside try"));
    assert!(actual.out.contains("this finally"));
}

#[test]
fn finally_should_run_before_return() {
    // finally should run after return.
    let actual =
        nu!("def aa [] { try { return 3 } finally { print 'this finally' } }; let x = aa; $x == 3");
    assert!(actual.out.contains("this finally"));
    assert!(actual.out.contains("true"));

    let actual = nu!(
        "def aa [] { try { 1 / 0 } catch { return 44 } finally { print 'this finally' } }; let x = aa; $x == 44"
    );
    assert!(actual.out.contains("this finally"));
    assert!(actual.out.contains("true"));
}

#[test]
fn return_statement_in_finally_should_be_used() {
    // finally should run before return.
    let actual = nu!("def aa [] { try { return 3 } finally { return 4 } }; let x = aa; $x == 4");
    assert!(actual.out.contains("true"));
}

#[test]
fn try_finally_with_variable() {
    // try failed with finally
    let actual = nu!("try { 1 / 0 } finally {|x| print $x.msg }");
    assert_eq!(actual.out, "Division by zero.");

    let actual = nu!("try { 3 } finally {|x| print ($x == 3) }");
    assert!(actual.out.contains("true"));
    assert!(actual.out.ends_with('3'));
}

#[test]
fn try_exit_runs_finally() {
    let actual = nu!("try { exit 3 } finally { print 'this finally' }");
    assert_eq!(actual.out, "this finally");
    assert_eq!(actual.status.code(), Some(3));

    // nested try with exit should run all finally block
    let actual = nu!("
    try {
        try {
            exit 3
        } finally { 
            print 'inner finally'
        }
    } finally {
        print 'outer finally'
    }");
    assert!(actual.out.contains("inner finally"));
    assert!(actual.out.contains("outer finally"));
    assert_eq!(actual.status.code(), Some(3));
}

#[test]
fn try_abort_not_run_finally() {
    let actual = nu!("try { exit 3 --abort} finally { print 'this finally' }");
    assert!(!actual.out.contains("this finally"));
    assert_eq!(actual.status.code(), Some(3));
}
#[test]
fn catch_finally_with_variable() {
    // try catch with finally
    let actual = nu!("try { 1 / 0 } catch { 33 } finally {|x| print ($x == 33) }");
    assert!(actual.out.contains("true"));
    assert!(actual.out.ends_with("33"));

    let actual = nu!(
        "try { 1 / 0 } catch { 33; error make 'err in catch' } finally {|x| print ($x.msg == 'err in catch')}"
    );
    assert_eq!(actual.out, "true");
}

#[test]
fn finally_should_not_run_before_try_finished() {
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        "
        with-env { FOO: 'bar' } {
            try { nu --testbin echo_env FOO } finally { print 'bb' }
        }
        "
    );
    assert_eq!(actual.out, "barbb")
}

#[test]
fn finally_should_not_run_before_catch_finished() {
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        "
        with-env { FOO: 'bar' } {
            try { 1 / 0 } catch { nu --testbin echo_env FOO } finally { print 'bb' }
        }
        "
    );
    assert_eq!(actual.out, "barbb")
}

#[test]
fn finally_should_not_run_twice_when_error_in_finally() {
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        r#"
        try {
            ^true
        } finally {
            print "inside finally"
            error make -u "oh no"
        }
        "#
    );
    assert_eq!(actual.out, "inside finally")
}

#[test]
fn try_wont_generate_extra_output() {
    let actual = nu!(
        experimental: vec!["pipefail".to_string()],
        "try { nu --testbin fail | is-empty } catch { 'here' }"
    );
    assert_eq!(actual.out, "here")
}

// ===========================================================================
// Interrupt/SIGINT regression tests
// ===========================================================================

/// A command that triggers the interrupt flag when run.
/// Used to deterministically test interrupt handling without actual SIGINT.
#[derive(Clone)]
struct TriggerInterrupt;

impl nu_protocol::engine::Command for TriggerInterrupt {
    fn name(&self) -> &str {
        "trigger-interrupt"
    }

    fn description(&self) -> &str {
        "Triggers an interrupt (for testing purposes)"
    }

    fn signature(&self) -> nu_protocol::Signature {
        nu_protocol::Signature::build("trigger-interrupt").input_output_types(vec![(
            nu_protocol::Type::Nothing,
            nu_protocol::Type::Nothing,
        )])
    }

    fn run(
        &self,
        engine_state: &nu_protocol::engine::EngineState,
        _stack: &mut nu_protocol::engine::Stack,
        _call: &nu_protocol::engine::Call<'_>,
        _input: nu_protocol::PipelineData,
    ) -> Result<nu_protocol::PipelineData, nu_protocol::ShellError> {
        engine_state.signals().trigger();
        Ok(nu_protocol::PipelineData::empty())
    }
}

/// A command that records when it runs, via a shared `AtomicBool`.
#[derive(Clone)]
struct RecordRan(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl nu_protocol::engine::Command for RecordRan {
    fn name(&self) -> &str {
        "record-ran"
    }

    fn description(&self) -> &str {
        "Sets a shared flag to record that this command ran (for testing purposes)"
    }

    fn signature(&self) -> nu_protocol::Signature {
        nu_protocol::Signature::build("record-ran").input_output_types(vec![(
            nu_protocol::Type::Nothing,
            nu_protocol::Type::Nothing,
        )])
    }

    fn run(
        &self,
        _engine_state: &nu_protocol::engine::EngineState,
        _stack: &mut nu_protocol::engine::Stack,
        _call: &nu_protocol::engine::Call<'_>,
        _input: nu_protocol::PipelineData,
    ) -> Result<nu_protocol::PipelineData, nu_protocol::ShellError> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(nu_protocol::PipelineData::empty())
    }
}

/// Deterministic test: verify the `finally` block runs when an interrupt
/// (`ShellError::Interrupted`) is raised inside the `try` block.
///
/// This simulates the scenario where Ctrl+C / SIGINT is pressed while a
/// long-running command runs inside `try { ... } finally { ... }`.
#[test]
fn try_finally_runs_on_interrupt() {
    let finally_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Set up real signals so that `trigger-interrupt` actually sets the flag.
    let interrupt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signals = nu_protocol::Signals::new(interrupt.clone());

    // Evaluate: try { trigger-interrupt } finally { record-ran }
    // `trigger-interrupt` sets the interrupt flag; the engine detects it and
    // should run the finally block before propagating the error.
    let result = test()
        .with_signals(signals)
        .add_command(TriggerInterrupt)
        .add_command(RecordRan(finally_ran.clone()))
        .run_raw("try { trigger-interrupt } finally { record-ran }");

    // The evaluation should result in an error (the interrupt was propagated).
    assert!(result.is_err(), "expected error after interrupt");

    // The finally block must have run.
    assert!(
        finally_ran.load(std::sync::atomic::Ordering::SeqCst),
        "finally block should run when the try block is interrupted"
    );
}

/// Variant: verify the `finally` block runs when interrupted inside a
/// `try { ... } catch { ... } finally { ... }` construct.
///
/// The interrupt should bypass the catch block and go directly to finally.
#[test]
fn try_catch_finally_finally_runs_on_interrupt() {
    let finally_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let catch_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let interrupt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signals = nu_protocol::Signals::new(interrupt.clone());

    let result = test()
        .with_signals(signals)
        .add_command(TriggerInterrupt)
        .add_command(RecordRan(finally_ran.clone()))
        .add_command({
            // Re-use RecordRan with a different flag for the catch block
            #[derive(Clone)]
            struct RecordCatchRan(std::sync::Arc<std::sync::atomic::AtomicBool>);
            impl nu_protocol::engine::Command for RecordCatchRan {
                fn name(&self) -> &str {
                    "record-catch-ran"
                }
                fn description(&self) -> &str {
                    "Records that catch ran"
                }
                fn signature(&self) -> nu_protocol::Signature {
                    nu_protocol::Signature::build("record-catch-ran").input_output_types(vec![(
                        nu_protocol::Type::Nothing,
                        nu_protocol::Type::Nothing,
                    )])
                }
                fn run(
                    &self,
                    _: &nu_protocol::engine::EngineState,
                    _: &mut nu_protocol::engine::Stack,
                    _: &nu_protocol::engine::Call<'_>,
                    _: nu_protocol::PipelineData,
                ) -> Result<nu_protocol::PipelineData, nu_protocol::ShellError> {
                    self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(nu_protocol::PipelineData::empty())
                }
            }
            RecordCatchRan(catch_ran.clone())
        })
        .run_raw("try { trigger-interrupt } catch { record-catch-ran } finally { record-ran }");

    assert!(result.is_err(), "expected error after interrupt");
    assert!(
        finally_ran.load(std::sync::atomic::Ordering::SeqCst),
        "finally block should run when interrupted (even with catch clause)"
    );
    // Interrupt bypasses catch, so catch should not have run
    assert!(
        !catch_ran.load(std::sync::atomic::Ordering::SeqCst),
        "catch block should not run for interrupt (interrupt bypasses catch)"
    );
}

/// On Unix: integration test using real SIGINT sent from the test process.
///
/// This tests the full end-to-end path: a nu script runs `sleep` (which polls
/// for signals internally), the test sends SIGINT to the nu subprocess, and the
/// `finally` block should still execute.
#[cfg(unix)]
#[test]
fn try_finally_runs_when_script_receives_sigint() {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let nu_binary = nu_test_support::fs::executable_path();
    // Use the builtin `sleep` command: it periodically checks for interrupts,
    // so it will reliably detect a SIGINT delivered to the process.
    let script = "try { sleep 5sec } finally { print 'finally_ran' }";

    let child = Command::new(nu_binary)
        .args(["--no-config-file", "--commands", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn nu subprocess");

    // Allow the script to start and reach the `sleep` command.
    std::thread::sleep(Duration::from_millis(300));

    // Send SIGINT to the nu subprocess from the test process.
    // Using `kill -2 <pid>` (SIGINT) via the external kill utility.
    let _status = Command::new("kill")
        .args(["-2", &child.id().to_string()])
        .status()
        .expect("failed to send SIGINT to nu subprocess");

    // Wait for the subprocess to finish (it should exit promptly after interrupt).
    let output = child
        .wait_with_output()
        .expect("failed to wait for nu subprocess");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("finally_ran"),
        "finally block should run when the script receives SIGINT; stdout was: {stdout:?}"
    );
}
