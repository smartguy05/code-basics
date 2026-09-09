use super::*;

#[cfg(windows)]
use cb_core::browser::framing;

/// Drive `call_tool` with the registry pointed at a file that does not exist —
/// which is the situation a client is in whenever no application is running,
/// and the one it hits most often.
fn answer(tool: &str) -> ToolAnswer {
    let previous = std::env::var_os("CB_BROWSER_INSTANCES_PATH");
    std::env::set_var(
        "CB_BROWSER_INSTANCES_PATH",
        r"C:\nowhere\no-such-registry.json",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");
    let answer = runtime.block_on(call_tool(tool, Value::Null, None));
    match previous {
        Some(value) => std::env::set_var("CB_BROWSER_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_BROWSER_INSTANCES_PATH"),
    }
    answer
}

/// The ordering that is the whole point: the name is checked **before** the
/// registry. `call_tool` forwards the tool name to the application, so without
/// this an unknown tool is answered by whatever the application *lookup* says —
/// and with no application running that is "start code-basics and retry", about
/// a tool that will never exist however many times it is retried.
#[test]
fn an_unknown_tool_is_refused_by_name_before_an_application_is_looked_for() {
    let answer = answer("browser_evaluate");
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("unknown_tool"));
    assert!(answer.text.contains("browser_evaluate"), "{}", answer.text);
}

/// And the check is only the *name*: an advertised tool still goes on to look
/// for an application, so a name guard cannot swallow the real refusal.
#[test]
fn an_advertised_tool_still_reports_that_no_application_is_running() {
    let answer = answer(cb_core::browser::tools::STATUS);
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("app_not_running"));
}

/// A fake application: one pipe, one connection, and whatever bytes the test
/// wants to answer with. Real `tokio` named pipes, so this exercises the actual
/// client path — connect, frame, decode, `parse_answer` — rather than a stand-in.
#[cfg(windows)]
async fn fake_application(name: &str, reply: Vec<u8>) -> tokio::task::JoinHandle<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ServerOptions;

    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(name)
        .expect("the test's own pipe");
    tokio::spawn(async move {
        server.connect().await.expect("the client connects");
        let mut server = server;
        let mut buffer = vec![0u8; 4096];
        let read = server.read(&mut buffer).await.expect("the request arrives");
        buffer.truncate(read);
        server
            .write_all(&reply)
            .await
            .expect("the reply is written");
        server.flush().await.ok();
        buffer
    })
}

#[cfg(windows)]
fn a_request() -> wire::Request {
    wire::Request {
        protocol: cb_core::browser::instances::PROTOCOL_VERSION,
        token: "t".repeat(64),
        tool: cb_core::browser::tools::STATUS.to_string(),
        arguments: Value::Null,
    }
}

#[cfg(windows)]
fn pipe_name(suffix: &str) -> String {
    format!(
        r"\\.\pipe\code-basics.browser.test.{}.{suffix}",
        std::process::id()
    )
}

/// The round trip, over a real pipe: the request is framed and sent, and the
/// application's reply comes back as the answer it encoded.
#[test]
fn a_reply_over_a_real_pipe_comes_back_as_the_answer_the_application_sent() {
    let name = pipe_name("roundtrip");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let sent = cb_core::browser::wire::answer_value(
            &Value::from(1),
            &ToolAnswer::ok("the page is ready"),
        );
        let server = fake_application(&name, framing::encode(&sent)).await;
        let answer = exchange(&name, &a_request()).await.expect("an answer");
        assert!(answer.ok);
        assert_eq!(answer.text, "the page is ready");

        // And what the application received really was our framed request.
        let received = server.await.unwrap();
        let line = String::from_utf8(received).unwrap();
        assert!(line.ends_with('\n'), "exactly one framed line: {line:?}");
        let decoded: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(
            decoded["method"],
            Value::from(cb_core::browser::wire::METHOD)
        );
        assert_eq!(decoded["params"]["tool"], Value::from("browser_status"));
    });
}

/// A reply is exactly one frame, and the client stops at the first complete
/// line. This pins the shape of that read: a second frame written in the same
/// breath does **not** become the answer, and — the part worth pinning — the
/// first one is not skipped in favour of it.
#[test]
fn the_first_complete_frame_is_the_answer_and_a_second_one_does_not_replace_it() {
    let name = pipe_name("twoframes");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut bytes = framing::encode(&cb_core::browser::wire::answer_value(
            &Value::from(1),
            &ToolAnswer::ok("first"),
        ));
        bytes.extend(framing::encode(&cb_core::browser::wire::answer_value(
            &Value::from(1),
            &ToolAnswer::ok("second"),
        )));
        let _server = fake_application(&name, bytes).await;
        let answer = exchange(&name, &a_request()).await.expect("an answer");
        assert_eq!(answer.text, "first");
    });
}

/// An application that closes without answering is *closed*, not a timeout and
/// not an empty page — the distinction `PipeFailure` exists to keep.
#[test]
fn an_application_that_answers_nothing_is_reported_as_closed() {
    let name = pipe_name("silent");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let _server = fake_application(&name, Vec::new()).await;
        let failure = exchange(&name, &a_request())
            .await
            .expect_err("no answer is a failure");
        assert!(matches!(failure, PipeFailure::Closed { .. }), "{failure:?}");
    });
}

/// And a reply that is not a JSON-RPC message at all is reported as malformed,
/// naming the problem, rather than as a browser that could not be reached.
#[test]
fn a_reply_that_is_not_a_message_is_malformed_and_not_unreachable() {
    let name = pipe_name("garbage");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let _server = fake_application(&name, b"this is not json at all\n".to_vec()).await;
        let failure = exchange(&name, &a_request())
            .await
            .expect_err("garbage is a failure");
        assert!(
            matches!(failure, PipeFailure::Malformed { .. }),
            "{failure:?}"
        );
    });
}
