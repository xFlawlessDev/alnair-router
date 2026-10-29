//! A loopback listener for providers that pin their redirect URI.
//!
//! Most logins come back to the router's own `/api/oauth/callback`, which the
//! server already serves. The ChatGPT/Codex login cannot: OpenAI registers the
//! callback per client, so the URL is fixed at
//! `http://localhost:1455/auth/callback` and the browser sends the code there
//! no matter what the router's address is.
//!
//! This module binds that address for the duration of one login. It answers
//! only the expected path, validates the `state` nonce strictly, and then
//! yields the authorization code to the caller. The listener is bound to
//! `127.0.0.1`, so nothing outside the machine can reach it.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::{Error, Result};

/// How long the listener waits for the browser before giving up.
///
/// The operator may have to sign in to ChatGPT first, so this is generous; the
/// login itself is pruned by the registry's own TTL after that.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The outcome of one connection to the loopback listener.
enum Outcome {
    /// The callback arrived with a valid state; carries the code.
    Code(String),
    /// The request was not the callback (a favicon probe, a health check).
    Ignore,
    /// The callback arrived but failed validation.
    Rejected(String),
}

/// Binds `port` on the loopback interface and waits for one valid callback.
///
/// Returns the authorization code. A port already in use is reported as a
/// configuration error the operator can act on: every other Codex login
/// (including the CLI's own) claims the same port while it is running.
pub async fn await_code(port: u16, path: &str, expected_state: &str) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| {
            Error::BadRequest(format!(
                "could not listen on 127.0.0.1:{port} for the OAuth callback ({error}); \
             close any other login using that port and start again"
            ))
        })?;

    let deadline = tokio::time::Instant::now() + CALLBACK_TIMEOUT;
    loop {
        let accepted = tokio::time::timeout_at(deadline, listener.accept()).await;
        let (stream, _peer) = match accepted {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(error)) => {
                return Err(Error::Upstream(format!(
                    "the OAuth callback listener failed: {error}"
                )));
            }
            Err(_) => {
                return Err(Error::BadRequest(
                    "the OAuth login timed out before the browser came back; start it again"
                        .to_string(),
                ));
            }
        };

        match handle(stream, path, expected_state).await {
            Outcome::Code(code) => return Ok(code),
            Outcome::Ignore => continue,
            Outcome::Rejected(error) => return Err(Error::BadRequest(error)),
        }
    }
}

/// Reads one request, answers it, and classifies the result.
async fn handle(mut stream: TcpStream, path: &str, expected_state: &str) -> Outcome {
    let mut buffer = [0u8; 8192];
    let read = match stream.read(&mut buffer).await {
        Ok(0) | Err(_) => return Outcome::Ignore,
        Ok(read) => read,
    };
    let head = String::from_utf8_lossy(&buffer[..read]);
    let Some(request_line) = head.lines().next() else {
        return Outcome::Ignore;
    };

    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Outcome::Ignore;
    };
    if method != "GET" {
        let _ = respond(&mut stream, 405, "Method Not Allowed").await;
        return Outcome::Ignore;
    }

    let (request_path, query) = target.split_once('?').unwrap_or((target, ""));
    if request_path != path {
        let _ = respond(&mut stream, 404, "Not Found").await;
        return Outcome::Ignore;
    }

    match validate(query, expected_state) {
        Ok(code) => {
            let _ = respond(
                &mut stream,
                200,
                "Signed in. You can close this window and return to the router.",
            )
            .await;
            Outcome::Code(code)
        }
        Err(error) => {
            let _ = respond(
                &mut stream,
                400,
                "Login failed — return to the router and retry.",
            )
            .await;
            Outcome::Rejected(error)
        }
    }
}

/// Extracts and validates the code from a callback query string.
fn validate(query: &str, expected_state: &str) -> std::result::Result<String, String> {
    let mut code = None;
    let mut state = None;
    let mut error = None;

    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }

    if let Some(error) = error {
        return Err(format!("the provider refused the login: {error}"));
    }
    match state.as_deref() {
        Some(state) if state == expected_state => {}
        Some(_) => return Err("OAuth state mismatch — the callback was rejected".to_string()),
        None => return Err("the OAuth callback carried no state and was rejected".to_string()),
    }
    code.filter(|code| !code.is_empty())
        .ok_or_else(|| "the OAuth callback carried no authorization code".to_string())
}

async fn respond(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let body = format!("<!doctype html><meta charset=utf-8><title>Alnair</title><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_state_yields_the_code() {
        assert_eq!(
            validate("code=abc&state=s1", "s1").unwrap(),
            "abc".to_string()
        );
    }

    #[test]
    fn a_state_mismatch_is_rejected() {
        let error = validate("code=abc&state=other", "s1").unwrap_err();
        assert!(error.contains("mismatch"), "{error}");
    }

    #[test]
    fn an_absent_state_is_rejected() {
        let error = validate("code=abc", "s1").unwrap_err();
        assert!(error.contains("no state"), "{error}");
    }

    #[test]
    fn a_provider_error_is_surfaced() {
        let error = validate("error=access_denied", "s1").unwrap_err();
        assert!(error.contains("access_denied"), "{error}");
    }

    #[tokio::test]
    async fn the_listener_answers_the_callback_and_returns_the_code() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let server = tokio::spawn(async move { await_code(port, "/auth/callback", "st").await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = reqwest::get(format!(
            "http://127.0.0.1:{port}/auth/callback?code=the-code&state=st"
        ))
        .await;

        let code = server
            .await
            .unwrap()
            .expect("a valid callback yields the code");
        assert_eq!(code, "the-code");
    }
}
