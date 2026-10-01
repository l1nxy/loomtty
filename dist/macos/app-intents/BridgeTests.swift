import AppIntents
import Foundation

@MainActor
private enum Host {
  static var requests: [[String: Any]] = []
  static var deferred: (UInt64, HostCompletion)?
  static var cancellations: [UInt64] = []
  static var deferNext = false
  static let records: [[String: String]] = [
    ["id": "first-run-terminal", "title": "中文 👩🏽‍💻", "directory": "/tmp/a b", "session": "work"],
    ["id": "second-terminal", "title": "Other", "directory": "/tmp/c", "session": "ops"],
  ]
  static func send(_ id: UInt64, _ completion: HostCompletion, _ value: [String: Any]) {
    let bytes = try! JSONSerialization.data(withJSONObject: value)
    bytes.withUnsafeBytes { completion(id, $0.bindMemory(to: UInt8.self).baseAddress, $0.count) }
  }
  static func receive(_ id: UInt64, _ data: Data, _ completion: HostCompletion) {
    let request = try! JSONSerialization.jsonObject(with: data) as! [String: Any]
    requests.append(request)
    if deferNext {
      deferNext = false
      deferred = (id, completion)
      return
    }
    switch request["action"] as! String {
    case "list": send(id, completion, ["terminals": records])
    case "new_window", "new_tab", "split":
      send(id, completion, ["terminals": [], "terminal": records[0]])
    case "input", "focus", "close":
      if records.contains(where: { $0["id"] == request["target"] as? String }) {
        send(id, completion, ["terminals": []])
      } else {
        send(id, completion, ["terminals": [], "error": "The target no longer exists"])
      }
    default: fatalError("Unexpected action")
    }
  }
}
private func submit(
  _ id: UInt64, _ bytes: UnsafePointer<UInt8>?, _ count: Int, _ completion: HostCompletion
) {
  let data = Data(bytes: bytes!, count: count)
  MainActor.assumeIsolated { Host.receive(id, data, completion) }
}
private func cancel(_ id: UInt64) {
  MainActor.assumeIsolated { Host.cancellations.append(id) }
}

@main
struct BridgeTests {
  @MainActor
  static func main() async throws {
    guard #available(macOS 13.0, *) else { return }
    do {
      _ = try await IntentBridge.request(IntentRequest(action: "list"))
      fatalError("Uninstalled host accepted request")
    } catch { precondition(error.localizedDescription == "loomtty is not ready") }
    loomAppIntentsInstall(submit, cancel)
    let query = TerminalQuery()
    let all = try await query.entities(matching: "")
    precondition(all.count == 2)
    let reordered = try await query.entities(for: [
      "second-terminal", "expired", "first-run-terminal",
    ])
    precondition(reordered.map(\.id) == ["second-terminal", "first-run-terminal"])
    let unicodeMatches = try await query.entities(matching: "中文")
    let directoryMatches = try await query.entities(matching: "/tmp/A B")
    precondition(unicodeMatches.count == 1 && directoryMatches.count == 1)

    let new = NewTerminalIntent()
    new.location = .tab
    new.parent = all[1]
    new.directory = "/tmp/a b"
    let created = try await new.perform()
    precondition(created.value?.id == "first-run-terminal")
    precondition(Host.requests.last?["target"] as? String == "second-terminal")
    new.location = .right
    new.parent = nil
    do {
      _ = try await new.perform()
      fatalError("Parentless split accepted")
    } catch { precondition(error.localizedDescription.contains("parent")) }
    new.parent = all[0]
    do {
      _ = try await new.perform()
      fatalError("Split directory ignored")
    } catch { precondition(error.localizedDescription.contains("inherit")) }
    new.directory = nil
    _ = try await new.perform()
    precondition(Host.requests.last?["action"] as? String == "split")

    let input = InputTextIntent()
    input.terminal = all[0]
    input.text = "printf '中文 👩🏽‍💻'\n\0"
    _ = try await input.perform()
    precondition(Host.requests.last?["input"] as? String == input.text)
    let before = Host.requests.count
    input.text = String(repeating: "中", count: 90_000)
    do {
      _ = try await input.perform()
      fatalError("Oversized input accepted")
    } catch { precondition(error.localizedDescription.contains("256 KiB")) }
    precondition(Host.requests.count == before)
    do {
      _ = try await IntentBridge.request(IntentRequest(action: "focus", target: "expired"))
      fatalError("Stale terminal accepted")
    } catch { precondition(error.localizedDescription.contains("no longer exists")) }

    Host.deferNext = true
    let task = Task { @MainActor in try await IntentBridge.request(IntentRequest(action: "list")) }
    for _ in 0..<100 {
      if Host.deferred != nil { break }
      try await Task.sleep(nanoseconds: 1_000_000)
    }
    precondition(Host.deferred != nil)
    task.cancel()
    do {
      _ = try await task.value
      fatalError("Cancellation was ignored")
    } catch { precondition(error is CancellationError) }
    let deferred = Host.deferred!
    precondition(Host.cancellations.contains(deferred.0))
    // A late reply must not resume an already-cancelled continuation.
    Host.send(deferred.0, deferred.1, ["terminals": Host.records])
    Host.deferred = nil
    Host.deferNext = true
    let pendingAtShutdown = Task { @MainActor in
      try await IntentBridge.request(IntentRequest(action: "list"))
    }
    for _ in 0..<100 {
      if Host.deferred != nil { break }
      try await Task.sleep(nanoseconds: 1_000_000)
    }
    precondition(Host.deferred != nil)
    loomAppIntentsShutdown()
    do {
      _ = try await pendingAtShutdown.value
      fatalError("Pending request survived shutdown")
    } catch { precondition(error.localizedDescription == "loomtty is quitting") }
    let shutdownReply = Host.deferred!
    Host.send(shutdownReply.0, shutdownReply.1, ["terminals": Host.records])
    do {
      _ = try await IntentBridge.request(IntentRequest(action: "list"))
      fatalError("Shutdown ignored")
    } catch { precondition(error.localizedDescription == "loomtty is not ready") }
    print(
      "App Intents: entity queries, actions, Unicode input, failures, cancellation and shutdown passed"
    )
  }
}
