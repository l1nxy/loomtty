use super::{
    Request, Source, Verb, WaitResult, Waiting, cocoa, intents,
    model::{Kind, Object},
};
use crate::macos::{MacApplication, native};
use loom_protocol::message::ClientMessage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use winit::event_loop::ActiveEventLoop;

thread_local! {
    static RESULTS: RefCell<HashMap<u64, Result<u64, String>>> = RefCell::default();
}
pub(crate) fn creation_result(token: u64, pane: Option<u64>, error: Option<String>) {
    if super::request(token).is_some_and(|request| request.verb == Verb::Split) {
        let result = match (pane, error) {
            (Some(pane), None) => Ok(pane),
            (_, error) => {
                Err(error.unwrap_or_else(|| "The server did not return a pane ID".into()))
            }
        };
        RESULTS.with(|results| {
            results.borrow_mut().insert(token, result);
        });
    }
}
fn send(app: &crate::app::App, message: ClientMessage) -> Result<(), String> {
    if !app.core.connected || app.core.pending_session_name.is_some() {
        return Err("The terminal connection is not ready".into());
    }
    app.core
        .server_tx
        .as_ref()
        .ok_or("The server connection is unavailable")?
        .try_send(message)
        .map_err(|error| match error {
            crossbeam_channel::TrySendError::Full(_) => {
                "The server input queue is full; retry the command".into()
            }
            crossbeam_channel::TrySendError::Disconnected(_) => "The server disconnected".into(),
        })
}
fn target(
    request: &Request,
    snapshot: &super::model::Snapshot,
    source: Source,
) -> Result<Object, String> {
    let id = request
        .target
        .as_ref()
        .ok_or("A target object is required")?;
    let object = snapshot
        .objects
        .get(id)
        .ok_or("The target no longer exists")?;
    let expected = match request.verb {
        Verb::NewTab | Verb::Focus if source == Source::AppIntents => Some(Kind::Terminal),
        Verb::NewTab | Verb::CloseWindow => Some(Kind::Window),
        Verb::CloseTab => Some(Kind::Tab),
        Verb::Split | Verb::Input | Verb::CloseTerminal => Some(Kind::Terminal),
        _ => None,
    };
    if expected.is_some_and(|kind| kind != object.kind) {
        return Err("The target object has the wrong type for this command".into());
    }
    Ok(object.clone())
}
impl MacApplication {
    pub(in crate::macos) fn update_scripting(&mut self) {
        self.update_automation(Source::AppleScript);
        if cfg!(any(feature = "macos-app-intents", test)) {
            self.update_automation(Source::AppIntents);
        }
    }

    fn automation(&mut self, source: Source) -> &mut super::Bridge {
        match source {
            Source::AppleScript => &mut self.scripting,
            Source::AppIntents => &mut self.intents,
        }
    }

    fn update_automation(&mut self, source: Source) {
        let enabled = source.enabled(
            self.active_index()
                .map(|i| &self.windows[i].core.config.window)
                .unwrap_or(&self.last_config.window),
        );
        let bridge = match source {
            Source::AppleScript => &mut self.scripting,
            Source::AppIntents => &mut self.intents,
        };
        let snapshot = bridge.capture(&self.windows, self.active, enabled, source);
        match source {
            Source::AppleScript => cocoa::publish(snapshot.clone()),
            Source::AppIntents => intents::publish(snapshot.clone()),
        }
        let now = Instant::now();
        let mut completions = Vec::new();
        bridge.waiting.retain_mut(|wait| {
            if super::request(wait.token).is_none() {
                RESULTS.with(|results| results.borrow_mut().remove(&wait.token));
                return false;
            }
            let failure = if !enabled {
                Some(format!("{} was disabled", source.label()))
            } else if now >= wait.deadline {
                Some("Terminal creation timed out; inspect the window before retrying".to_owned())
            } else if !snapshot.objects.contains_key(&wait.tab) {
                Some("The new tab was closed".to_owned())
            } else {
                None
            };
            if let Some(error) = failure {
                completions.push((wait.token, Err(error)));
                return false;
            }
            let result = match &mut wait.result {
                WaitResult::Tab { window } => snapshot
                    .objects
                    .get(&wait.tab)
                    .filter(|tab| tab.ready && tab.focused.is_some())
                    .and_then(|_| {
                        if *window && source == Source::AppleScript {
                            snapshot
                                .windows
                                .iter()
                                .find(|id| snapshot.objects[*id].tabs.contains(&wait.tab))
                                .cloned()
                        } else {
                            Some(wait.tab.clone())
                        }
                    })
                    .map(|id| Ok(Some(id))),
                WaitResult::Split { parent, pane } => {
                    if !snapshot.objects.contains_key(parent) {
                        Some(Err(
                            "The target terminal was closed or its connection changed".into(),
                        ))
                    } else {
                        if let Some(reply) =
                            RESULTS.with(|results| results.borrow_mut().remove(&wait.token))
                        {
                            match reply {
                                Ok(id) => *pane = Some(id),
                                Err(error) => {
                                    completions.push((wait.token, Err(error)));
                                    return false;
                                }
                            }
                        }
                        pane.and_then(|pane| {
                            snapshot
                                .objects
                                .get(&wait.tab)?
                                .terminals
                                .iter()
                                .find(|id| snapshot.objects[*id].pane_id == Some(pane))
                                .cloned()
                        })
                        .map(|id| Ok(Some(id)))
                    }
                }
            };
            if let Some(result) = result {
                completions.push((wait.token, result));
                false
            } else {
                true
            }
        });
        for (token, result) in completions {
            RESULTS.with(|results| {
                results.borrow_mut().remove(&token);
            });
            super::finish(token, result);
        }
    }

    pub(in crate::macos) fn execute_script(&mut self, token: u64, event_loop: &ActiveEventLoop) {
        let Some(request) = super::request(token) else {
            return;
        };
        self.update_scripting();
        let source = Source::for_token(token);
        let snapshot = match source {
            Source::AppleScript => cocoa::snapshot(),
            Source::AppIntents => intents::snapshot(),
        };
        if !snapshot.enabled {
            super::finish(token, Err(format!("{} is disabled", source.label())));
            return;
        }
        let result = self.apply_script(token, &request, &snapshot, event_loop);
        self.update_scripting();
        if let Err(error) = result {
            super::finish(token, Err(error));
        } else if !self
            .automation(source)
            .waiting
            .iter()
            .any(|wait| wait.token == token)
        {
            super::finish(token, Ok(None));
        }
    }

    fn apply_script(
        &mut self,
        token: u64,
        request: &Request,
        snapshot: &super::model::Snapshot,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        let source = Source::for_token(token);
        if request.verb == Verb::ListTerminals {
            return Ok(());
        }
        if matches!(request.verb, Verb::NewWindow | Verb::NewTab) {
            let directory = request
                .directory
                .as_ref()
                .map(|directory| {
                    let path = std::path::PathBuf::from(directory);
                    if !path.is_absolute() || !path.is_dir() {
                        Err("Working directory must be an existing absolute folder path".to_owned())
                    } else {
                        Ok(path)
                    }
                })
                .transpose()?;
            if request.verb == Verb::NewTab && request.target.is_some() {
                self.active = Some(target(request, snapshot, source)?.window_id);
            }
            let before = self.windows.len();
            self.create_window(event_loop, request.verb == Verb::NewTab, false, directory);
            if self.windows.len() == before {
                return Err("Could not create a terminal window".into());
            }
            let window = self
                .windows
                .last()
                .and_then(|app| app.window.as_ref())
                .ok_or("Window initialization failed")?
                .id();
            let tab = self.automation(source).ids.tab(window);
            self.automation(source).waiting.push(Waiting {
                token,
                tab,
                deadline: Instant::now() + Duration::from_secs(30),
                result: WaitResult::Tab {
                    window: request.verb == Verb::NewWindow,
                },
            });
            return Ok(());
        }
        let object = target(request, snapshot, source)?;
        let index = self
            .windows
            .iter()
            .position(|app| {
                app.window
                    .as_ref()
                    .is_some_and(|window| window.id() == object.window_id)
            })
            .ok_or("The target window no longer exists")?;
        let tab_id = self.automation(source).ids.tab(object.window_id);
        let app = &mut self.windows[index];
        if !source.enabled(&app.core.config.window) {
            return Err(format!(
                "{} is disabled for the target window",
                source.label()
            ));
        }
        if matches!(
            request.verb,
            Verb::Input | Verb::Split | Verb::CloseTerminal | Verb::Focus
        ) && app.modal_captures_keyboard()
        {
            return Err("Dismiss the terminal's dialog or overlay before controlling it".into());
        }
        match request.verb {
            Verb::Input => {
                let pane_id = object.pane_id.ok_or("Expected a terminal")?;
                send(
                    app,
                    ClientMessage::Input {
                        pane_id,
                        data: request
                            .input
                            .as_deref()
                            .unwrap_or_default()
                            .as_bytes()
                            .to_vec(),
                        input_seq: 0,
                    },
                )?;
            }
            Verb::Split => {
                let pane_id = object.pane_id.ok_or("Expected a terminal")?;
                let tab = tab_id;
                send(
                    app,
                    ClientMessage::CreatePaneAt {
                        pane_id,
                        below: request.direction == "down",
                        request_id: token,
                    },
                )?;
                self.automation(source).waiting.push(Waiting {
                    token,
                    tab,
                    deadline: Instant::now() + Duration::from_secs(30),
                    result: WaitResult::Split {
                        parent: object.id,
                        pane: None,
                    },
                });
            }
            Verb::Focus => {
                if let Some(pane_id) = object.pane_id {
                    let workspace = app
                        .core
                        .workspaces
                        .workspaces
                        .iter()
                        .position(|workspace| workspace.all_pane_ids().contains(&pane_id))
                        .ok_or("The target terminal no longer exists")?;
                    send(app, ClientMessage::FocusPane { pane_id })?;
                    app.focus_workspace_pane_local(workspace, pane_id);
                    app.schedule_redraw();
                }
                if let Some(window) = app
                    .window
                    .as_ref()
                    .and_then(|window| native::native_window(window))
                {
                    if window.isMiniaturized() {
                        window.deminiaturize(None);
                    }
                    window.makeKeyAndOrderFront(None);
                    let mtm =
                        objc2::MainThreadMarker::new().expect("scripting runs on the main thread");
                    #[allow(deprecated)]
                    objc2_app_kit::NSApplication::sharedApplication(mtm)
                        .activateIgnoringOtherApps(true);
                }
                self.active = Some(object.window_id);
            }
            Verb::CloseTerminal => send(
                app,
                ClientMessage::ClosePane {
                    pane_id: object.pane_id.ok_or("Expected a terminal")?,
                },
            )?,
            Verb::CloseTab => self.close_window(object.window_id, event_loop),
            Verb::CloseWindow => {
                for tab in object.tabs {
                    if let Some(tab) = snapshot.objects.get(&tab) {
                        self.close_window(tab.window_id, event_loop);
                    }
                }
            }
            _ => unreachable!("creation handled above"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_intents_parent_is_a_live_terminal_and_capabilities_are_independent() {
        let mut config = loom_config::schema::WindowConfig {
            macos_applescript: false,
            ..Default::default()
        };
        assert!(!Source::AppleScript.enabled(&config));
        assert!(Source::AppIntents.enabled(&config));
        config.macos_app_intents = false;
        config.macos_applescript = true;
        assert!(Source::AppleScript.enabled(&config));
        assert!(!Source::AppIntents.enabled(&config));

        let mut snapshot = super::super::model::Snapshot::default();
        let object = Object {
            id: "t".into(),
            kind: Kind::Terminal,
            name: "shell".into(),
            tabs: vec![],
            terminals: vec![],
            selected: None,
            focused: None,
            cwd: String::new(),
            session: "s".into(),
            window_id: winit::window::WindowId::dummy(),
            pane_id: Some(1),
            ready: true,
        };
        snapshot.objects.insert("t".into(), object.clone());
        let request = Request {
            verb: Verb::NewTab,
            target: Some("t".into()),
            input: None,
            directory: None,
            direction: "right".into(),
        };
        assert!(target(&request, &snapshot, Source::AppIntents).is_ok());
        assert!(target(&request, &snapshot, Source::AppleScript).is_err());
        snapshot.objects.clear();
        let mut surviving_window = object;
        surviving_window.id = "w".into();
        surviving_window.kind = Kind::Window;
        snapshot.objects.insert("w".into(), surviving_window);
        assert!(target(&request, &snapshot, Source::AppIntents).is_err());
    }
    #[test]
    fn input_reports_backpressure_instead_of_silently_dropping_text() {
        let mut app = crate::app::App::new(loom_config::config::LoomConfig::default(), "test");
        let (tx, _rx) = crossbeam_channel::bounded(1);
        tx.send(ClientMessage::FocusPane { pane_id: 1 }).unwrap();
        app.core.server_tx = Some(tx);
        app.core.connected = true;
        assert!(
            send(
                &app,
                ClientMessage::Input {
                    pane_id: 1,
                    data: b"hello".to_vec(),
                    input_seq: 0
                }
            )
            .unwrap_err()
            .contains("queue is full")
        );
    }
    #[test]
    fn pending_session_switch_rejects_input_to_old_grid() {
        let mut app = crate::app::App::new(loom_config::config::LoomConfig::default(), "test");
        let (tx, rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);
        app.core.connected = true;
        app.core.pending_session_name = Some("another".into());
        assert!(
            send(
                &app,
                ClientMessage::Input {
                    pane_id: 1,
                    data: b"hello".to_vec(),
                    input_seq: 0
                }
            )
            .is_err()
        );
        assert!(rx.is_empty());
    }
}
