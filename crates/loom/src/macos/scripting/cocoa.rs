use super::model::{Kind, Object, Snapshot};
use super::{Request, Verb};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ClassBuilder, Sel};
use objc2::{AnyThread, ClassType, DefinedClass, Message, define_class, msg_send, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSScriptCommand, NSScriptObjectSpecifier,
    NSScriptSuiteRegistry, NSString, NSUniqueIDSpecifier,
};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
struct State {
    snapshot: Snapshot,
    next: u64,
    pending: BTreeMap<u64, (Retained<NSScriptCommand>, Request)>,
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }

pub fn publish(snapshot: Snapshot) {
    STATE.with(|state| state.borrow_mut().snapshot = snapshot);
}
pub fn snapshot() -> Snapshot {
    STATE.with(|state| state.borrow().snapshot.clone())
}
fn error(command: &NSScriptCommand, message: &str) {
    command.setScriptErrorNumber(-1708);
    command.setScriptErrorString(Some(&NSString::from_str(message)));
}
fn allowed() -> bool {
    let enabled = STATE.with(|state| state.borrow().snapshot.enabled);
    if !enabled && let Some(command) = NSScriptCommand::currentCommand() {
        error(
            &command,
            "AppleScript is disabled by window.macos_applescript",
        );
    }
    enabled
}
fn object(id: &str) -> Option<Object> {
    if !allowed() {
        return None;
    }
    let object = STATE.with(|state| state.borrow().snapshot.objects.get(id).cloned());
    if object.is_none()
        && let Some(command) = NSScriptCommand::currentCommand()
    {
        command.setScriptErrorNumber(-1728);
        command.setScriptErrorString(Some(&NSString::from_str(
            "The loomtty object no longer exists",
        )));
    }
    object
}
fn string(id: &str, field: impl FnOnce(&Object) -> &str) -> Retained<NSString> {
    NSString::from_str(object(id).as_ref().map(field).unwrap_or(""))
}
fn list(ids: &[String]) -> Retained<NSArray<AnyObject>> {
    NSArray::from_retained_slice(&ids.iter().filter_map(|id| proxy(id)).collect::<Vec<_>>())
}
fn specifier(id: &str) -> Option<Retained<NSScriptObjectSpecifier>> {
    let object = object(id)?;
    let description = NSScriptSuiteRegistry::sharedScriptSuiteRegistry()
        .classDescriptionWithAppleEventCode(u32::from_be_bytes(*b"capp"))?;
    // Every proxy also belongs to the application's flattened collection. An
    // absolute ID specifier survives tab moves without retaining a dead parent.
    Some(
        unsafe {
            NSUniqueIDSpecifier::initWithContainerClassDescription_containerSpecifier_key_uniqueID(
                NSUniqueIDSpecifier::alloc(),
                &description,
                None,
                &NSString::from_str(object.kind.key()),
                &NSString::from_str(id),
            )
        }
        .into_super(),
    )
}

// These are separate Cocoa classes so each has the correct scripting class
// description. The shared immutable getters never borrow the Rust application.
macro_rules! proxy_class {
    ($name:ident) => {
        define_class!(
            #[unsafe(super = NSObject)]
            #[ivars = String]
            struct $name;
            unsafe impl NSObjectProtocol for $name {}
            impl $name {
                #[unsafe(method_id(uniqueID))]
                fn unique_id(&self) -> Retained<NSString> { NSString::from_str(self.ivars()) }
                #[unsafe(method_id(name))]
                fn name(&self) -> Retained<NSString> { string(self.ivars(), |o| &o.name) }
                #[unsafe(method_id(workingDirectory))]
                fn cwd(&self) -> Retained<NSString> { string(self.ivars(), |o| &o.cwd) }
                #[unsafe(method_id(sessionName))]
                fn session(&self) -> Retained<NSString> { string(self.ivars(), |o| &o.session) }
                #[unsafe(method_id(tabs))]
                fn tabs(&self) -> Retained<NSArray<AnyObject>> { list(&object(self.ivars()).map(|o| o.tabs).unwrap_or_default()) }
                #[unsafe(method_id(terminals))]
                fn terminals(&self) -> Retained<NSArray<AnyObject>> { list(&object(self.ivars()).map(|o| o.terminals).unwrap_or_default()) }
                #[unsafe(method_id(selectedTab))]
                fn selected(&self) -> Option<Retained<AnyObject>> { object(self.ivars()).and_then(|o| o.selected).and_then(|id| proxy(&id)) }
                #[unsafe(method_id(focusedTerminal))]
                fn focused(&self) -> Option<Retained<AnyObject>> { object(self.ivars()).and_then(|o| o.focused).and_then(|id| proxy(&id)) }
                #[unsafe(method(ready))]
                fn ready(&self) -> bool { object(self.ivars()).is_some_and(|o| o.ready) }
                #[unsafe(method_id(objectSpecifier))]
                fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> { specifier(self.ivars()) }
            }
        );
        impl $name {
            fn new(id: String) -> Retained<Self> {
                let this = Self::alloc().set_ivars(id);
                // SAFETY: NSObject's init has no extra subclass requirements.
                unsafe { msg_send![super(this), init] }
            }
        }
    }
}
proxy_class!(LoomScriptWindow);
proxy_class!(LoomScriptTab);
proxy_class!(LoomScriptTerminal);

fn proxy(id: &str) -> Option<Retained<AnyObject>> {
    Some(match object(id)?.kind {
        Kind::Window => LoomScriptWindow::new(id.to_owned()).into(),
        Kind::Tab => LoomScriptTab::new(id.to_owned()).into(),
        Kind::Terminal => LoomScriptTerminal::new(id.to_owned()).into(),
    })
}
fn id(value: Retained<AnyObject>) -> Result<String, String> {
    let value = if let Some(specifier) = value.downcast_ref::<NSScriptObjectSpecifier>() {
        specifier
            .objectsByEvaluatingSpecifier()
            .ok_or("The target no longer exists")?
    } else {
        value
    };
    if let Some(value) = value.downcast_ref::<LoomScriptWindow>() {
        return Ok(value.ivars().clone());
    }
    if let Some(value) = value.downcast_ref::<LoomScriptTab>() {
        return Ok(value.ivars().clone());
    }
    if let Some(value) = value.downcast_ref::<LoomScriptTerminal>() {
        return Ok(value.ivars().clone());
    }
    Err("Expected a loomtty window, tab, or terminal object".into())
}
fn text(value: Retained<AnyObject>) -> Result<String, String> {
    value
        .downcast_ref::<NSString>()
        .map(|s| s.to_string())
        .ok_or_else(|| "Expected text".into())
}
fn parse(command: &NSScriptCommand) -> Result<Request, String> {
    let code = command.commandDescription().appleEventCode().to_be_bytes();
    let verb = match &code {
        b"nwin" => Verb::NewWindow,
        b"ntab" => Verb::NewTab,
        b"splt" => Verb::Split,
        b"focs" => Verb::Focus,
        b"ctrm" => Verb::CloseTerminal,
        b"ctab" => Verb::CloseTab,
        b"cwin" => Verb::CloseWindow,
        b"inpt" => Verb::Input,
        _ => return Err("Unknown loomtty command".into()),
    };
    let args = command.evaluatedArguments();
    let get = |key: &str| {
        args.as_ref()
            .and_then(|args| args.objectForKey(&NSString::from_str(key)))
    };
    let target = if matches!(
        verb,
        Verb::Split | Verb::Focus | Verb::CloseTerminal | Verb::CloseTab | Verb::CloseWindow
    ) {
        command.directParameter().map(id).transpose()?
    } else {
        get("target").map(id).transpose()?
    };
    let input = if verb == Verb::Input {
        Some(text(
            command.directParameter().ok_or("Input text is required")?,
        )?)
    } else {
        None
    };
    let directory = get("directory").map(text).transpose()?;
    let direction = get("direction")
        .map(text)
        .transpose()?
        .unwrap_or_else(|| "right".into());
    if verb == Verb::Split && direction != "right" && direction != "down" {
        return Err("Split direction must be right or down".into());
    }
    // Vec<u8> may expand to a MessagePack integer array. Leave room for
    // encoding overhead below the protocol's 1 MiB control-frame ceiling.
    if input.as_ref().is_some_and(|text| text.len() > 256 * 1024) {
        return Err("Input is limited to 256 KiB per command".into());
    }
    Ok(Request {
        verb,
        target,
        input,
        directory,
        direction,
    })
}
extern "C" fn perform(this: &NSScriptCommand, _sel: Sel) -> *mut AnyObject {
    if !allowed() {
        error(this, "AppleScript is disabled by window.macos_applescript");
        return std::ptr::null_mut();
    }
    let request = match parse(this) {
        Ok(request) => request,
        Err(message) => {
            error(this, &message);
            return std::ptr::null_mut();
        }
    };
    let token = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.pending.len() >= 64 {
            return None;
        }
        state.next += 1;
        let token = state.next;
        state.pending.insert(token, (this.retain(), request));
        Some(token)
    });
    if let Some(token) = token {
        this.suspendExecution();
        super::super::native::dispatch(super::super::Command::Script(token));
    } else {
        error(this, "Too many pending AppleScript commands");
    }
    std::ptr::null_mut()
}
pub fn request(token: u64) -> Option<Request> {
    STATE.with(|state| {
        state
            .borrow()
            .pending
            .get(&token)
            .map(|(_, request)| request.clone())
    })
}
pub fn finish(token: u64, result: Result<Option<String>, String>) {
    let pending = STATE.with(|state| state.borrow_mut().pending.remove(&token));
    if let Some((command, _)) = pending {
        let value = match result {
            Ok(Some(id)) => match proxy(&id) {
                Some(value) => Some(value),
                None => {
                    error(&command, "The result no longer exists");
                    None
                }
            },
            Ok(None) => None,
            Err(message) => {
                error(&command, &message);
                None
            }
        };
        // SAFETY: result matches the dictionary (object reference or no result).
        // No TLS borrow or application callback is held across Cocoa resumption.
        unsafe { command.resumeExecutionWithResult(value.as_deref()) };
    }
}
pub fn cancel_all(message: &str) {
    let tokens: Vec<_> = STATE.with(|state| state.borrow().pending.keys().copied().collect());
    for token in tokens {
        finish(token, Err(message.into()));
    }
}

extern "C" fn handles(_this: &AnyObject, _sel: Sel, _app: &NSApplication, key: &NSString) -> Bool {
    Bool::new(matches!(
        key.to_string().as_str(),
        "loomWindows" | "loomTabs" | "loomTerminals" | "loomFrontWindow"
    ))
}
extern "C" fn collection(_this: &AnyObject, selector: Sel) -> *mut NSArray<AnyObject> {
    let values = if allowed() {
        STATE.with(|state| {
            let state = state.borrow();
            if selector == sel!(loomWindows) {
                state.snapshot.windows.clone()
            } else if selector == sel!(loomTabs) {
                state.snapshot.tabs.clone()
            } else {
                state.snapshot.terminals.clone()
            }
        })
    } else {
        Vec::new()
    };
    Retained::autorelease_ptr(list(&values))
}
extern "C" fn front(_this: &AnyObject, _sel: Sel) -> *mut AnyObject {
    let id = STATE.with(|state| state.borrow().snapshot.front.clone());
    id.and_then(|id| proxy(&id))
        .map(Retained::autorelease_ptr)
        .unwrap_or(std::ptr::null_mut())
}
pub fn install(delegate: &mut ClassBuilder) {
    // Register classes before NSScriptSuiteRegistry loads the bundle dictionary.
    let _ = (
        LoomScriptWindow::class(),
        LoomScriptTab::class(),
        LoomScriptTerminal::class(),
    );
    let mut command = ClassBuilder::new(c"LoomScriptCommand", NSScriptCommand::class())
        .expect("scripting command class");
    // SAFETY: documented Cocoa selectors, no added ivars or layout changes.
    unsafe {
        command.add_method(
            sel!(performDefaultImplementation),
            perform as extern "C" fn(_, _) -> _,
        );
        delegate.add_method(
            sel!(application:delegateHandlesKey:),
            handles as extern "C" fn(_, _, _, _) -> _,
        );
        for selector in [sel!(loomWindows), sel!(loomTabs), sel!(loomTerminals)] {
            delegate.add_method(selector, collection as extern "C" fn(_, _) -> _);
        }
        delegate.add_method(sel!(loomFrontWindow), front as extern "C" fn(_, _) -> _);
    }
    command.register();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run from the test .app assembled by dist/macos/test-scripting.sh. Cocoa
    // loads sdef files from the main bundle, not arbitrary loadSuites bundles.
    // This uses Foundation only: it does not launch a GUI or send Apple events.
    #[test]
    #[ignore = "requires the scripting test app bundle"]
    fn bundled_dictionary_matches_cocoa_objects_and_commands() {
        assert_eq!(
            std::env::var("LOOM_SCRIPTING_TEST_BUNDLE").as_deref(),
            Ok("1")
        );
        let mut delegate = ClassBuilder::new(c"LoomScriptTestDelegate", NSObject::class()).unwrap();
        install(&mut delegate);
        delegate.register();
        let registry = NSScriptSuiteRegistry::sharedScriptSuiteRegistry();
        for code in [*b"capp", *b"cwin", *b"Ltab", *b"Ltrm"] {
            assert!(
                registry
                    .classDescriptionWithAppleEventCode(u32::from_be_bytes(code))
                    .is_some(),
                "missing class {code:?}"
            );
        }
        for code in [
            *b"nwin", *b"ntab", *b"splt", *b"focs", *b"ctrm", *b"ctab", *b"cwin", *b"inpt",
        ] {
            let description = registry
                .commandDescriptionWithAppleEventClass_andAppleEventCode(
                    u32::from_be_bytes(*b"Loom"),
                    u32::from_be_bytes(code),
                )
                .expect("command is registered");
            assert_eq!(
                description.commandClassName().to_string(),
                "LoomScriptCommand"
            );
        }
        let window_id = winit::window::WindowId::dummy();
        let mut snapshot = Snapshot {
            enabled: true,
            ..Snapshot::default()
        };
        snapshot.objects.insert(
            "terminal-42".into(),
            Object {
                id: "terminal-42".into(),
                kind: Kind::Terminal,
                name: "中文 shell".into(),
                tabs: vec![],
                terminals: vec![],
                selected: None,
                focused: None,
                cwd: "/tmp/with spaces".into(),
                session: "script-test".into(),
                window_id,
                pane_id: Some(7),
                ready: true,
            },
        );
        snapshot.terminals.push("terminal-42".into());
        publish(snapshot);
        let terminal = proxy("terminal-42").unwrap();
        let name: Retained<NSString> = unsafe { msg_send![&terminal, name] };
        assert_eq!(name.to_string(), "中文 shell");
        let descriptor = specifier("terminal-42").unwrap();
        assert_eq!(descriptor.key().to_string(), "loomTerminals");
        // Serialize the reference without resolving its application root. The
        // reverse Cocoa resolver requires a live NSApplication; this harness
        // intentionally does not create one or run a GUI event loop.
        let event_descriptor = descriptor
            .descriptor()
            .expect("serializable object reference");
        // AEKeyword is a u32; the typed bindings gate this selector behind
        // CoreServices although the method itself is provided by Foundation.
        let desired_class: Option<Retained<objc2_foundation::NSAppleEventDescriptor>> = unsafe {
            msg_send![&event_descriptor, descriptorForKeyword: u32::from_be_bytes(*b"want")]
        };
        assert_eq!(
            desired_class.unwrap().typeCodeValue(),
            u32::from_be_bytes(*b"Ltrm")
        );
        let unique_id: Option<Retained<objc2_foundation::NSAppleEventDescriptor>> = unsafe {
            msg_send![&event_descriptor, descriptorForKeyword: u32::from_be_bytes(*b"seld")]
        };
        assert_eq!(
            unique_id.unwrap().stringValue().unwrap().to_string(),
            "terminal-42"
        );
        let description = registry
            .commandDescriptionWithAppleEventClass_andAppleEventCode(
                u32::from_be_bytes(*b"Loom"),
                u32::from_be_bytes(*b"inpt"),
            )
            .unwrap();
        assert!(
            description
                .argumentNames()
                .iter()
                .any(|key| key.to_string() == "target")
        );
        let command =
            NSScriptCommand::initWithCommandDescription(NSScriptCommand::alloc(), &description);
        let args = objc2_foundation::NSDictionary::from_slices(
            &[&*NSString::from_str("target")],
            &[&*terminal],
        );
        unsafe {
            command.setDirectParameter(Some(&NSString::from_str("echo 'hello'\n")));
            command.setArguments(Some(&args));
        }
        let parsed = parse(&command).unwrap();
        assert_eq!(parsed.target.as_deref(), Some("terminal-42"));
        assert_eq!(parsed.input.as_deref(), Some("echo 'hello'\n"));
        unsafe {
            command.setDirectParameter(Some(&NSString::from_str(&"x".repeat(256 * 1024 + 1))));
        }
        assert!(parse(&command).unwrap_err().contains("256 KiB"));
        publish(Snapshot::default());
        assert!(proxy("terminal-42").is_none());
    }
}
