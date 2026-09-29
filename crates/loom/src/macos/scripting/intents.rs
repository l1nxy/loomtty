//! In-process Swift ↔ Rust bridge. All callbacks run on the main thread and
//! queue work; App Intent callbacks never borrow the live application.
use super::{
    Request, Verb,
    model::{Kind, Object, Snapshot},
};
use objc2::MainThreadMarker;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};

type Completion = unsafe extern "C" fn(u64, *const u8, usize);
type Submit = unsafe extern "C" fn(u64, *const u8, usize, Completion);
type Cancel = extern "C" fn(u64);

struct Pending {
    context: u64,
    completion: Completion,
    request: Request,
}
#[derive(Default)]
struct State {
    snapshot: Snapshot,
    next: u64,
    pending: BTreeMap<u64, Pending>,
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }

#[cfg(feature = "macos-app-intents")]
unsafe extern "C" {
    fn loom_app_intents_install(submit: Submit, cancel: Cancel);
    fn loom_app_intents_shutdown();
}

pub(in crate::macos) fn start(enabled: bool) {
    publish(Snapshot {
        enabled,
        ..Snapshot::default()
    });
    #[cfg(feature = "macos-app-intents")]
    // SAFETY: called on the main thread before handing control to AppKit. The
    // function pointers live for the process lifetime and use the documented
    // byte-buffer ABI in Bridge.swift; buffers are borrowed only during calls.
    unsafe {
        if supported() {
            loom_app_intents_install(submit, cancel);
        }
    };
    #[cfg(not(feature = "macos-app-intents"))]
    let _callbacks: (Submit, Cancel) = (submit, cancel);
}
pub(in crate::macos) fn stop() {
    let tokens: Vec<_> = STATE.with(|s| s.borrow().pending.keys().copied().collect());
    for token in tokens {
        finish(token, Err("loomtty is quitting".into()));
    }
    publish(Snapshot::default());
    #[cfg(feature = "macos-app-intents")]
    // SAFETY: same main-thread and lifetime contract as install above.
    unsafe {
        if supported() {
            loom_app_intents_shutdown();
        }
    };
}

/// Used only by the bundle builder. The metadata is embedded in the exact
/// executable being packaged, so it cannot come from a stale Cargo OUT_DIR.
pub(crate) fn metadata() -> Result<&'static str, &'static str> {
    #[cfg(feature = "macos-app-intents")]
    {
        Ok(include_str!(concat!(
            env!("OUT_DIR"),
            "/LoomAppIntents.swiftconstvalues"
        )))
    }
    #[cfg(not(feature = "macos-app-intents"))]
    {
        Err(
            "Rebuild loomtty with --features macos-app-intents, or package a development app with --without-app-intents",
        )
    }
}

#[cfg(feature = "macos-app-intents")]
fn supported() -> bool {
    objc2_foundation::NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        >= 13
}

pub(super) fn publish(snapshot: Snapshot) {
    STATE.with(|s| s.borrow_mut().snapshot = snapshot);
}
pub(super) fn snapshot() -> Snapshot {
    STATE.with(|s| s.borrow().snapshot.clone())
}
pub(super) fn request(token: u64) -> Option<Request> {
    STATE.with(|s| s.borrow().pending.get(&token).map(|p| p.request.clone()))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    NewWindow,
    NewTab,
    Split,
    Focus,
    Input,
    Close,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    action: Action,
    target: Option<String>,
    input: Option<String>,
    directory: Option<String>,
    direction: Option<String>,
}
fn parse(bytes: &[u8]) -> Result<Request, String> {
    let wire: WireRequest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid App Intent request")?;
    let verb = match wire.action {
        Action::List => Verb::ListTerminals,
        Action::NewWindow => Verb::NewWindow,
        Action::NewTab => Verb::NewTab,
        Action::Split => Verb::Split,
        Action::Focus => Verb::Focus,
        Action::Input => Verb::Input,
        Action::Close => Verb::CloseTerminal,
    };
    if matches!(
        verb,
        Verb::Split | Verb::Focus | Verb::Input | Verb::CloseTerminal
    ) && wire.target.is_none()
    {
        return Err("A terminal is required".into());
    }
    if verb == Verb::Input && wire.input.is_none() {
        return Err("Input text is required".into());
    }
    if wire
        .input
        .as_ref()
        .is_some_and(|input| input.len() > 256 * 1024)
    {
        return Err("Input is limited to 256 KiB per command".into());
    }
    let direction = wire.direction.unwrap_or_else(|| "right".into());
    if verb == Verb::Split && direction != "right" && direction != "down" {
        return Err("Split direction must be right or down".into());
    }
    if wire
        .directory
        .as_ref()
        .is_some_and(|s| s.len() > 16 * 1024 || s.contains('\0'))
    {
        return Err("Invalid working directory".into());
    }
    Ok(Request {
        verb,
        target: wire.target,
        input: wire.input,
        directory: wire.directory,
        direction,
    })
}

#[derive(Serialize)]
struct Terminal {
    id: String,
    title: String,
    directory: String,
    session: String,
}
impl From<&Object> for Terminal {
    fn from(o: &Object) -> Self {
        Self {
            id: o.id.clone(),
            title: o.name.clone(),
            directory: o.cwd.clone(),
            session: o.session.clone(),
        }
    }
}
#[derive(Default, Serialize)]
struct Response {
    terminals: Vec<Terminal>,
    terminal: Option<Terminal>,
    error: Option<String>,
}
fn response(
    snapshot: &Snapshot,
    request: &Request,
    result: Result<Option<String>, String>,
) -> Response {
    let id = match result {
        Ok(id) => id,
        Err(error) => {
            return Response {
                error: Some(error),
                ..Response::default()
            };
        }
    };
    if request.verb == Verb::ListTerminals {
        return Response {
            terminals: snapshot
                .terminals
                .iter()
                .filter_map(|id| snapshot.objects.get(id))
                .map(Terminal::from)
                .collect(),
            ..Response::default()
        };
    }
    let terminal = id
        .as_ref()
        .and_then(|id| snapshot.objects.get(id))
        .and_then(|o| {
            if o.kind == Kind::Terminal {
                Some(o)
            } else {
                o.focused.as_ref().and_then(|id| snapshot.objects.get(id))
            }
        })
        .map(Terminal::from);
    if matches!(request.verb, Verb::NewWindow | Verb::NewTab | Verb::Split) && terminal.is_none() {
        return Response {
            error: Some("The new terminal no longer exists".into()),
            ..Response::default()
        };
    }
    Response {
        terminal,
        ..Response::default()
    }
}
fn complete(context: u64, completion: Completion, response: Response) {
    let bytes = serde_json::to_vec(&response).expect("serializing a string-only response");
    // SAFETY: Swift supplied this process-lifetime callback. It copies the
    // bytes before returning; no TLS borrow is held across its invocation.
    unsafe { completion(context, bytes.as_ptr(), bytes.len()) };
}
pub(super) fn finish(token: u64, result: Result<Option<String>, String>) {
    let pending = STATE.with(|s| s.borrow_mut().pending.remove(&token));
    if let Some(pending) = pending {
        let value = response(&snapshot(), &pending.request, result);
        complete(pending.context, pending.completion, value);
    }
}
fn enqueue(context: u64, completion: Completion, request: Request) -> Result<u64, String> {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if !state.snapshot.enabled {
            return Err("App Intents is disabled by window.macos_app_intents".into());
        }
        if state.pending.len() >= 64 {
            return Err("Too many pending App Intents".into());
        }
        state.next = state
            .next
            .checked_add(1)
            .filter(|n| *n < (1 << 63))
            .ok_or("App Intent request IDs exhausted")?;
        let token = state.next | (1 << 63);
        state.pending.insert(
            token,
            Pending {
                context,
                completion,
                request,
            },
        );
        Ok(token)
    })
}
unsafe extern "C" fn submit(context: u64, bytes: *const u8, len: usize, completion: Completion) {
    let result = (|| {
        if MainThreadMarker::new().is_none() {
            return Err("App Intents must run on the main thread".into());
        }
        // JSON escaping can expand a 256 KiB UTF-8 input to six times its size.
        if bytes.is_null() || len > 2 * 1024 * 1024 {
            return Err("App Intent request is too large".into());
        }
        // SAFETY: Bridge.swift lends its encoded Data for this call only.
        let request = parse(unsafe { std::slice::from_raw_parts(bytes, len) })?;
        enqueue(context, completion, request)
    })();
    match result {
        Ok(token) => super::super::native::dispatch(super::super::Command::Script(token)),
        Err(error) => complete(
            context,
            completion,
            Response {
                error: Some(error),
                ..Response::default()
            },
        ),
    }
}
extern "C" fn cancel(context: u64) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    remove_context(context);
    // Wake the event loop to release creation waiters, without retrying or
    // undoing an operation which may already have reached the server.
    super::super::native::dispatch(super::super::Command::AccessibilityRefresh);
}

fn remove_context(context: u64) {
    STATE.with(|s| s.borrow_mut().pending.retain(|_, p| p.context != context));
}

#[cfg(test)]
mod tests {
    use super::super::Source;
    use super::*;
    use serde_json::{Value, json};
    thread_local! { static REPLIES: RefCell<Vec<(u64, Value)>> = const { RefCell::new(Vec::new()) }; }
    unsafe extern "C" fn reply(id: u64, bytes: *const u8, len: usize) {
        let value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(bytes, len) }).unwrap();
        STATE.with(|s| {
            assert!(
                s.try_borrow_mut().is_ok(),
                "completion retained a state borrow"
            )
        });
        REPLIES.with(|s| s.borrow_mut().push((id, value)));
    }
    fn reset() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        REPLIES.with(|s| s.borrow_mut().clear());
        publish(Snapshot {
            enabled: true,
            ..Snapshot::default()
        });
    }
    fn terminal(id: &str) -> Object {
        Object {
            id: id.into(),
            kind: Kind::Terminal,
            name: "中文 👩🏽‍💻".into(),
            tabs: vec![],
            terminals: vec![],
            selected: None,
            focused: None,
            cwd: "/tmp/a b".into(),
            session: "session".into(),
            window_id: winit::window::WindowId::dummy(),
            pane_id: Some(1),
            ready: true,
        }
    }
    #[test]
    fn input_is_literal_utf8_and_rejects_invalid_or_oversized_requests() {
        let text = "printf '中文 👩🏽‍💻'\n\0";
        let request = parse(
            &serde_json::to_vec(&json!({"action":"input","target":"terminal","input":text}))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(request.input.as_deref(), Some(text));
        for value in [
            json!({"action":"input", "target":"terminal"}),
            json!({"action":"input", "input":"text"}),
            json!({"action":"split", "target":"terminal", "direction":"left"}),
            json!({"action":"new_window", "directory":"a\0b"}),
            json!({"action":"list", "unknown":true}),
            json!({"action":"input", "target":"terminal", "input":"中".repeat(90_000)}),
        ] {
            assert!(parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
    #[test]
    fn completion_can_reenter_and_pending_tokens_do_not_overlap_applescript() {
        reset();
        let request = parse(br#"{"action":"list"}"#).unwrap();
        let token = enqueue(42, reply, request).unwrap();
        assert_eq!(Source::for_token(token), Source::AppIntents);
        assert_eq!(Source::for_token(42), Source::AppleScript);
        let mut snapshot = snapshot();
        snapshot.terminals.push("t".into());
        snapshot.objects.insert("t".into(), terminal("t"));
        publish(snapshot);
        finish(token, Ok(None));
        finish(token, Err("late duplicate reply".into()));
        assert!(super::request(token).is_none());
        REPLIES.with(|s| {
            let s = s.borrow();
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].0, 42);
            assert_eq!(s[0].1["terminals"][0]["title"], "中文 👩🏽‍💻");
        });
    }
    #[test]
    fn disabled_and_full_queues_reject_without_enqueuing_and_shutdown_completes_once() {
        reset();
        for n in 0..64 {
            enqueue(n, reply, parse(br#"{"action":"list"}"#).unwrap()).unwrap();
        }
        assert!(
            enqueue(65, reply, parse(br#"{"action":"list"}"#).unwrap())
                .unwrap_err()
                .contains("Too many")
        );
        // Use the Rust completion path; the Swift runtime belongs to the main
        // thread and is tested by test.sh instead of a Rust test worker.
        let tokens: Vec<_> = STATE.with(|s| s.borrow().pending.keys().copied().collect());
        for token in tokens {
            finish(token, Err("quitting".into()));
        }
        publish(Snapshot::default());
        assert!(
            enqueue(66, reply, parse(br#"{"action":"list"}"#).unwrap())
                .unwrap_err()
                .contains("disabled")
        );
        assert!(STATE.with(|s| s.borrow().pending.is_empty()));
        assert_eq!(REPLIES.with(|s| s.borrow().len()), 64);
    }
    #[test]
    fn cancelling_one_request_invalidates_late_replies_without_cancelling_other_requests() {
        reset();
        let a = enqueue(1, reply, parse(br#"{"action":"list"}"#).unwrap()).unwrap();
        let b = enqueue(2, reply, parse(br#"{"action":"list"}"#).unwrap()).unwrap();
        remove_context(1);
        assert!(request(a).is_none());
        assert!(request(b).is_some());
        finish(a, Ok(None));
        finish(b, Ok(None));
        assert_eq!(
            REPLIES.with(|s| s.borrow().iter().map(|(id, _)| *id).collect::<Vec<_>>()),
            vec![2]
        );
    }
    #[test]
    fn creation_resolves_the_created_tab_instead_of_a_different_selected_tab() {
        let mut snapshot = Snapshot::default();
        let mut created = terminal("created-tab");
        created.kind = Kind::Tab;
        created.focused = Some("created-terminal".into());
        let mut window = terminal("window");
        window.kind = Kind::Window;
        window.focused = Some("other-terminal".into());
        for object in [
            created,
            window,
            terminal("created-terminal"),
            terminal("other-terminal"),
        ] {
            snapshot.objects.insert(object.id.clone(), object);
        }
        let request = parse(br#"{"action":"new_window"}"#).unwrap();
        let response = response(&snapshot, &request, Ok(Some("created-tab".into())));
        assert_eq!(response.terminal.unwrap().id, "created-terminal");
        assert!(
            super::response(&snapshot, &request, Ok(Some("closed".into())))
                .error
                .is_some()
        );
    }
}
