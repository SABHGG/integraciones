//! Suite de pruebas del sandbox (tareas 2.1–2.5).
//!
//! Bloques trampa: bucle infinito, `throw`, agotamiento de memoria, Promise
//! colgante y servidor HTTP lento. Todos deben terminar en error del ejecutor
//! sin colgar el worker.
//!
//! El servidor HTTP de prueba usa `std::net::TcpListener` en un hilo con una
//! respuesta HTTP escrita a mano, sin dependencias adicionales de servidor.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use sandbox::{SandboxConfig, SandboxError, run_block};
use serde_json::json;

/// Configuración base para los tests: timeout y límite de memoria holgados.
fn base_cfg(timeout_ms: u64) -> SandboxConfig {
    SandboxConfig {
        timeout_ms,
        memory_limit_bytes: 64 * 1024 * 1024,
        allow_domains: None,
        credential_resolver: Arc::new(|_name: String| None),
        http_client: reqwest::Client::new(),
    }
}

/// Levanta un servidor HTTP mínimo que atiende exactamente una conexión.
///
/// `delay` retrasa la respuesta para simular un servidor lento. Devuelve la URL
/// base (`http://127.0.0.1:puerto`) y el join handle del hilo.
fn spawn_http_server(
    content_type: &str,
    body: &str,
    delay: Duration,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    let content_type = content_type.to_string();
    let body = body.to_string();

    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Leer la petición completa (suficiente para GET/POST pequeños).
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://{addr}"), handle)
}

// ---------------------------------------------------------------------------
// 2.2 Bloques síncronos y asíncronos
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_block_returns_output() {
    let code = r#"
        export default function(input, ctx) {
            return { doubled: input.n * 2 };
        }
    "#;
    let out = run_block(code, json!({ "n": 21 }), base_cfg(5_000))
        .await
        .expect("sync block should succeed");
    assert_eq!(out, json!({ "doubled": 42 }));
}

#[tokio::test]
async fn async_block_promise_resolves() {
    let code = r#"
        export default function(input, ctx) {
            return Promise.resolve({ ok: true, got: input.n });
        }
    "#;
    let out = run_block(code, json!({ "n": 7 }), base_cfg(5_000))
        .await
        .expect("resolved promise should succeed");
    assert_eq!(out, json!({ "ok": true, "got": 7 }));
}

#[tokio::test]
async fn async_function_block_resolves() {
    let code = r#"
        export default async function(input, ctx) {
            return { sum: input.a + input.b };
        }
    "#;
    let out = run_block(code, json!({ "a": 1, "b": 2 }), base_cfg(5_000))
        .await
        .expect("async function should succeed");
    assert_eq!(out, json!({ "sum": 3 }));
}

#[tokio::test]
async fn native_throw_becomes_structured_error() {
    let code = r#"
        export default function(input, ctx) {
            throw new Error("boom");
        }
    "#;
    let err = run_block(code, json!({}), base_cfg(5_000))
        .await
        .expect_err("throw should fail the run");
    match err {
        SandboxError::JsError { message } => assert!(
            message.contains("boom"),
            "message should carry the thrown text, got: {message}"
        ),
        other => panic!("expected JsError, got {other:?}"),
    }
}

#[tokio::test]
async fn fresh_isolate_has_no_residual_state() {
    let writer = r#"
        export default function(input, ctx) {
            globalThis.__leak = 42;
            return input;
        }
    "#;
    run_block(writer, json!(1), base_cfg(5_000))
        .await
        .expect("writer block should succeed");

    let reader = r#"
        export default function(input, ctx) {
            return typeof globalThis.__leak === "undefined" ? "clean" : "leaked";
        }
    "#;
    let out = run_block(reader, json!(null), base_cfg(5_000))
        .await
        .expect("reader block should succeed");
    assert_eq!(out, json!("clean"));
}

// ---------------------------------------------------------------------------
// 2.1 Interrupt handler / límites
// ---------------------------------------------------------------------------

#[tokio::test]
async fn infinite_loop_is_interrupted_by_timeout() {
    let code = r#"
        export default function(input, ctx) {
            while (true) {}
        }
    "#;
    let err = run_block(code, json!({}), base_cfg(300))
        .await
        .expect_err("infinite loop must be interrupted");
    assert!(matches!(err, SandboxError::Timeout), "got {err:?}");
}

#[tokio::test]
async fn dangling_promise_is_interrupted_by_timeout() {
    let code = r#"
        export default function(input, ctx) {
            return new Promise(() => {});
        }
    "#;
    let err = run_block(code, json!({}), base_cfg(300))
        .await
        .expect_err("dangling promise must be interrupted");
    assert!(matches!(err, SandboxError::Timeout), "got {err:?}");
}

#[tokio::test]
async fn memory_exhaustion_maps_to_memory_limit() {
    let code = r#"
        export default function(input, ctx) {
            const chunks = [];
            while (true) {
                chunks.push(new Array(4096).fill(0));
            }
        }
    "#;
    let mut cfg = base_cfg(10_000);
    cfg.memory_limit_bytes = 1024 * 1024; // 1 MiB
    let err = run_block(code, json!({}), cfg)
        .await
        .expect_err("memory exhaustion must fail the run");
    assert!(
        matches!(err, SandboxError::MemoryLimit),
        "expected MemoryLimit, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 2.3 Host functions de ctx
// ---------------------------------------------------------------------------

#[tokio::test]
async fn crypto_helpers_are_available() {
    let code = r#"
        export default function(input, ctx) {
            return {
                sha: ctx.crypto.sha256("hello"),
                hmac: ctx.crypto.hmacSha256("key", "hello"),
                b64: ctx.crypto.base64Encode("hello"),
                back: ctx.crypto.base64Decode("aGVsbG8="),
            };
        }
    "#;
    let out = run_block(code, json!({}), base_cfg(5_000))
        .await
        .expect("crypto helpers should work");
    assert_eq!(
        out,
        json!({
            "sha": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            "hmac": "9307b3b915efb5171ff14d8cb55fbcc798c6c0ef1456d66ded1a6aa723a58b7b",
            "b64": "aGVsbG8=",
            "back": "hello",
        })
    );
}

#[tokio::test]
async fn json_helpers_are_available() {
    let code = r#"
        export default function(input, ctx) {
            const parsed = ctx.json.parse('{"b":2}');
            return {
                stringified: ctx.json.stringify({ a: 1, n: parsed.b }),
            };
        }
    "#;
    let out = run_block(code, json!({}), base_cfg(5_000))
        .await
        .expect("json helpers should work");
    assert_eq!(out["stringified"], json!(r#"{"a":1,"n":2}"#));
}

#[tokio::test]
async fn log_host_function_does_not_fail_the_run() {
    let code = r#"
        export default function(input, ctx) {
            ctx.log("info", "hello from block");
            ctx.log("warn", "careful");
            return "logged";
        }
    "#;
    let out = run_block(code, json!({}), base_cfg(5_000))
        .await
        .expect("log should not fail the run");
    assert_eq!(out, json!("logged"));
}

#[tokio::test]
async fn credential_resolver_is_exposed() {
    let code = r#"
        export default function(input, ctx) {
            return { secret: ctx.credential("api-key"), missing: ctx.credential("nope") };
        }
    "#;
    let mut cfg = base_cfg(5_000);
    cfg.credential_resolver = Arc::new(|name: String| {
        if name == "api-key" {
            Some("s3cr3t".to_string())
        } else {
            None
        }
    });
    let out = run_block(code, json!({}), cfg)
        .await
        .expect("credential lookup should work");
    assert_eq!(out, json!({ "secret": "s3cr3t", "missing": null }));
}

// ---------------------------------------------------------------------------
// 2.4 ctx.http.request
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_request_within_allowlist_ok() {
    let (url, server) = spawn_http_server("application/json", r#"{"ok":true}"#, Duration::ZERO);
    let code = r#"
        export default async function(input, ctx) {
            return await ctx.http.request({ url: input.url, method: "GET" });
        }
    "#;
    let mut cfg = base_cfg(5_000);
    cfg.allow_domains = Some(vec!["127.0.0.1".to_string()]);
    let out = run_block(code, json!({ "url": url }), cfg)
        .await
        .expect("allowed domain should work");
    assert_eq!(out, json!({ "ok": true }));
    let _ = server.join();
}

#[tokio::test]
async fn http_text_response_is_returned_as_string() {
    let (url, server) = spawn_http_server("text/plain", "pong", Duration::ZERO);
    let code = r#"
        export default async function(input, ctx) {
            return await ctx.http.request({ url: input.url });
        }
    "#;
    let out = run_block(code, json!({ "url": url }), base_cfg(5_000))
        .await
        .expect("text response should work");
    assert_eq!(out, json!("pong"));
    let _ = server.join();
}

#[tokio::test]
async fn http_request_outside_allowlist_is_denied() {
    // La petición se deniega antes de conectar, así que el servidor nunca
    // recibe la conexión: no se hace `join` para no bloquear el hilo de test.
    let (url, _server) = spawn_http_server("application/json", r#"{"ok":true}"#, Duration::ZERO);
    let code = r#"
        export default async function(input, ctx) {
            return await ctx.http.request({ url: input.url, method: "GET" });
        }
    "#;
    let mut cfg = base_cfg(5_000);
    cfg.allow_domains = Some(vec!["example.com".to_string()]);
    let err = run_block(code, json!({ "url": url }), cfg)
        .await
        .expect_err("domain outside allowlist must be denied");
    match err {
        SandboxError::HttpDenied { domain } => {
            assert_eq!(domain, "127.0.0.1", "denied domain should be reported");
        }
        other => panic!("expected HttpDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn http_request_to_slow_server_times_out() {
    let (url, server) = spawn_http_server(
        "application/json",
        r#"{"ok":true}"#,
        Duration::from_millis(1_500),
    );
    let code = r#"
        export default async function(input, ctx) {
            return await ctx.http.request({ url: input.url, method: "GET" });
        }
    "#;
    let err = run_block(code, json!({ "url": url }), base_cfg(300))
        .await
        .expect_err("slow server must time out");
    let message = format!("{err:?}");
    assert!(
        matches!(err, SandboxError::Timeout | SandboxError::HostError { .. }),
        "expected timeout-ish error, got {message}"
    );
    let _ = server.join();
}
