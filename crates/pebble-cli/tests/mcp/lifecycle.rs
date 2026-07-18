use super::*;

#[cfg(unix)]
#[test]
fn blocked_repository_index_stops_within_two_seconds_of_eof()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new()?;
    initialize_with_cli(&fixture)?;
    let bin = fixture.root.join("bin");
    let ready = fixture.root.join("git-ready");
    fs::create_dir(&bin)?;
    let git = bin.join("git");
    fs::write(
        &git,
        format!(
            "#!/bin/sh\n: > '{}'\nwhile kill -0 \"$PPID\" 2>/dev/null; do sleep 0.05; done\n",
            ready.display()
        ),
    )?;
    fs::set_permissions(&git, fs::Permissions::from_mode(0o700))?;

    let path = std::env::var_os("PATH").ok_or("missing PATH")?;
    let mut server_path = bin.into_os_string();
    server_path.push(":");
    server_path.push(path);
    let mut child = Command::new(env!("CARGO_BIN_EXE_pebble"))
        .arg("serve")
        .current_dir(&fixture.repository)
        .env("HOME", &fixture.home)
        .env("USERPROFILE", &fixture.home)
        .env("PATH", server_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let input = child.stdin.take().ok_or("missing server stdin")?;
    let output = BufReader::new(child.stdout.take().ok_or("missing server stdout")?);
    let mut server = Server {
        child,
        input,
        output,
    };
    server.initialize()?;
    serde_json::to_writer(
        &mut server.input,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "repository_index",
                "arguments": {"repository": fixture.repository}
            }
        }),
    )?;
    server.input.write_all(b"\n")?;
    server.input.flush()?;
    wait_for_file(&ready)?;

    drop(server.input);
    let started = Instant::now();
    loop {
        if let Some(status) = server.child.try_wait()? {
            assert!(status.success(), "server failed after EOF");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "blocked EOF shutdown took {:?}",
                started.elapsed()
            );
            return Ok(());
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "blocked server exceeded the EOF shutdown ceiling"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn saturated_notifications_do_not_starve_target_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new()?;
    initialize_with_cli(&fixture)?;
    let bin = fixture.root.join("cancel-bin");
    let ready = fixture.root.join("cancel-ready");
    fs::create_dir(&bin)?;
    let git = bin.join("git");
    fs::write(
        &git,
        format!(
            "#!/bin/sh\n: > '{}'\nwhile kill -0 \"$PPID\" 2>/dev/null; do sleep 0.05; done\n",
            ready.display()
        ),
    )?;
    fs::set_permissions(&git, fs::Permissions::from_mode(0o700))?;
    let path = std::env::var_os("PATH").ok_or("missing PATH")?;
    let mut server_path = bin.into_os_string();
    server_path.push(":");
    server_path.push(path);
    let mut child = Command::new(env!("CARGO_BIN_EXE_pebble"))
        .arg("serve")
        .current_dir(&fixture.repository)
        .env("HOME", &fixture.home)
        .env("USERPROFILE", &fixture.home)
        .env("PATH", server_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().ok_or("missing server stdin")?;
    let mut output = BufReader::new(child.stdout.take().ok_or("missing server stdout")?);
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2025-11-25",
                "capabilities":{},
                "clientInfo":{"name":"pebble-test","version":"1"}
            }
        }),
    )?;
    input.write_all(b"\n")?;
    input.flush()?;
    let mut line = Vec::new();
    output.read_until(b'\n', &mut line)?;
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"repository_index",
                "arguments":{"repository":fixture.repository}
            }
        }),
    )?;
    input.write_all(b"\n")?;
    input.flush()?;
    wait_for_file(&ready)?;
    for sequence in 0..64 {
        serde_json::to_writer(
            &mut input,
            &json!({
                "jsonrpc":"2.0",
                "method":"notifications/unknown",
                "params":{"sequence":sequence}
            }),
        )?;
        input.write_all(b"\n")?;
    }
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":2,"reason":"saturated"}
        }),
    )?;
    input.write_all(b"\n")?;
    input.flush()?;

    line.clear();
    output.read_until(b'\n', &mut line)?;
    let response: Value = serde_json::from_slice(&line)?;
    assert_eq!(response["id"], 2);
    assert_eq!(response["error"]["code"], -32_800);
    drop(input);
    let started = Instant::now();
    while child.try_wait()?.is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancelled server did not stop promptly"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(unix)]
fn wait_for_file(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    while !path.exists() {
        if started.elapsed() >= Duration::from_secs(2) {
            return Err("blocking git command did not start".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
