//! A real-network smoke test for [`cb_core::behavioral::replay`].
//!
//! The unit tests in `replay_tests.rs` deliberately keep the socket out of
//! `cb-core`. This integration test is the one place that proves the blocking
//! [`reqwest`] path actually works end to end — [`await_ready`] polls a server
//! until it is up, and [`send`] records a real response — without any GUI, app,
//! or fixed port. It binds `127.0.0.1:0`, so it is deterministic and cannot
//! collide with anything else on the machine.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cb_core::behavioral::{await_ready, send, HttpRequestSpec, Readiness};

/// Bind an ephemeral localhost port and serve a canned `200 {}` JSON response to
/// every connection. Returns the base url. The server thread loops accepting
/// connections (readiness plus each replayed request is its own connect) and is
/// left to die with the test process.
fn spawn_canned_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();

    thread::spawn(move || {
        // Bounded so a wedged test cannot spin forever; comfortably more than
        // the readiness poll plus the handful of requests this test drives.
        for _ in 0..64 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let body = "{}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
                Err(_) => break,
            }
        }
    });

    format!("http://127.0.0.1:{port}")
}

#[test]
fn await_ready_then_send_against_a_real_localhost_server() {
    let base = spawn_canned_server();
    let url = format!("{base}/");

    let readiness = Readiness {
        method: "GET".into(),
        url: url.clone(),
        expect_status: 200,
        timeout: Duration::from_secs(2),
        poll_interval: Duration::from_millis(50),
    };
    await_ready(&readiness).expect("server should become ready within the timeout");

    let request = HttpRequestSpec {
        name: "get-root".into(),
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
    };
    let recorded = send(&request).expect("request should succeed");

    assert_eq!(recorded.status, 200);
    assert_eq!(recorded.body, "{}");
    assert!(
        recorded
            .content_type
            .as_deref()
            .unwrap_or_default()
            .contains("json"),
        "content_type was {:?}",
        recorded.content_type
    );
}

#[test]
fn await_ready_times_out_when_nothing_listens() {
    // Bind and immediately drop, so the port is (almost certainly) closed.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let readiness = Readiness {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/"),
        expect_status: 200,
        timeout: Duration::from_millis(300),
        poll_interval: Duration::from_millis(50),
    };
    assert!(
        await_ready(&readiness).is_err(),
        "a closed port should never be reported ready"
    );
}
