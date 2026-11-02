# Connection lifecycle and resource audit

Reviewed both desktop implementations, their tunnel/process ownership, readiness and
manual latency paths, notification/settings callers, proxy configuration, QR decoding,
application discovery, and the associated tests. This is a targeted code audit, not a
native-platform soak test or a measured CPU/RSS benchmark.

## Fixed

| Finding | Change |
| --- | --- |
| Settings tests could overwrite real profiles and change login startup registration. | Isolate unit-test configuration by process/thread and suppress OS autostart writes in tests. |
| One failed periodic HTTP probe changed a live tunnel to reconnecting. | Removed the 10-second background probe subscription and duplicate retry probes on both platforms. Manual ping remains. |
| DNS success or any success log could mark an unverified tunnel connected. | Only the explicit verified-ready event (or SSH authentication event) changes readiness. Late readiness cannot undo Disconnect. |
| Three request errors could trigger recovery even with a live core. | Recovery starts only after the worker reports that it has exited and completed cleanup. |
| A failed startup HTTP check killed the core and removed routes. | Keep the existing core and retry initial verification after a 30-second delay. Verification stops after its first success. Disconnect remains cancellable during verification. |
| Linux's supervisor waited for stdin EOF even when Xray had died. | Wait for the core and watch parent EOF concurrently, without polling. Preserve the exit status and reap the owned core on shutdown. |
| Windows cleanup killed all processes named `xray.exe`, including other applications' cores and latency probes. | Remove global taskkill calls and use the owned child handle and `kill_on_drop`. Remove the destructive test of global cleanup. |
| Reconnect scheduling pretended a stopped worker was still active. | Preserve actual worker state, retry only when it is stopped, retain retry limits, and cancel queued retries when the setting is disabled. |
| Windows Add/import/Edit/Delete and unavailable Ping controls retained active colors. | Choose the explicit SVG color from the same enabled state used by the action. Match cached inactive icon colors to theme colors on both platforms. |
| SSH connection metrics leaked after early writes failed or a relay future was cancelled. | A small drop guard balances the active counter on every exit path. |
| Linux temporary Xray configuration could expose credentials or follow an existing symlink. | Create it exclusively with mode `0600`; clean up failed writes. A pre-existing path is rejected rather than overwritten. |
| Proxy setup silently ignored command failures and enabled settings before writing the target. | Check command results, enable last, and report setup failure instead of claiming success. |
| Latency parsing performed byte-at-a-time socket reads. | Read bounded chunks, scan only new bytes and the delimiter boundary, and retain the 8 KiB header limit. |
| QR decoding copied an owned grayscale image and accepted zero/truncated dimensions. | Consume the image buffer where possible and validate dimensions before conversion. |
| App sorting repeatedly allocated lowercase labels. | Cache each sorting key once. |
| Unused Xray access-log parser and probe-only state remained. | Remove them and replace obsolete probe tests with lifecycle regressions. |

No dependencies were added. Existing dependencies were retained where they still have
callers; dropping GUI/network features without platform measurements would risk regressions.

## Connection behavior

Xray continues handling new outbound requests during temporary network outages. An HTTP
endpoint failing is not proof that its process or every route is broken. The app no longer
closes a running tunnel in response to ping/DNS/request errors. Disconnect and confirmed
Exit still stop it and restore proxy/routes. A real core crash can still stop traffic;
auto-restart retains its existing five-attempt limit. No application can guarantee service
while its remote server or network is unavailable.

The connected indicator records successful initial verification; it is not a continuous
reachability guarantee. Manual Ping refreshes the displayed latency. Initial verification
still depends on the existing HTTP 204 endpoint and can remain pending if that endpoint
is blocked even when other destinations work.

## Validation

- Linux Rust regression suite, including lifecycle, cancellation, QR and HTTP framing tests.
- Windows crate library, binary and integration tests compiled and run on Linux.
- `python3 scripts/test-supervisor.py`: fake core exit and parent disconnect, without
  privileges, real routes, or a server; also added to Linux CI.
- Both crates checked with locked offline dependencies; whitespace validation.

Native Windows rendering/WinTun/registry effects and a live privileged Linux TUN session
need platform testing. The installed-core integration tests remain opt-in. No claim of
measured RAM savings is made: removing six background probes per connected minute and
redundant allocations is verified from the code, while actual RSS depends heavily on the
renderer, Xray configuration, and traffic.
