import Foundation

// Buffers are borrowed for the duration of the call. Rust and Swift copy before
// returning, and both sides call these functions only on the main thread.
typealias HostCompletion = @Sendable @convention(c) (UInt64, UnsafePointer<UInt8>?, Int) -> Void
typealias HostSubmit =
  @Sendable @convention(c) (UInt64, UnsafePointer<UInt8>?, Int, HostCompletion) -> Void
typealias HostCancel = @Sendable @convention(c) (UInt64) -> Void

struct TerminalRecord: Codable, Sendable {
  let id: String
  let title: String
  let directory: String
  let session: String
}
struct IntentRequest: Encodable, Sendable {
  let action: String
  var target: String? = nil
  var input: String? = nil
  var directory: String? = nil
  var direction: String? = nil
}
struct IntentResponse: Decodable, Sendable {
  let terminals: [TerminalRecord]
  let terminal: TerminalRecord?
  let error: String?
}
struct LoomIntentError: LocalizedError {
  let message: String
  var errorDescription: String? { message }
}

@MainActor
enum IntentBridge {
  private static var submit: HostSubmit?
  private static var cancel: HostCancel?
  private static var next: UInt64 = 0
  private struct Pending {
    let continuation: CheckedContinuation<IntentResponse, any Error>
    let timeout: Task<Void, Never>
  }
  private static var pending: [UInt64: Pending] = [:]

  static func install(submit: HostSubmit, cancel: HostCancel) {
    self.submit = submit
    self.cancel = cancel
  }
  static func shutdown() {
    for id in Array(pending.keys) { fail(id, LoomIntentError(message: "loomtty is quitting")) }
    submit = nil
    cancel = nil
  }
  private static func fail(_ id: UInt64, _ error: any Error) {
    guard let waiter = pending.removeValue(forKey: id) else { return }
    waiter.timeout.cancel()
    cancel?(id)
    waiter.continuation.resume(throwing: error)
  }
  static func complete(_ id: UInt64, data: Data?) {
    guard let waiter = pending.removeValue(forKey: id) else { return }
    waiter.timeout.cancel()
    do {
      guard let data else { throw LoomIntentError(message: "Invalid terminal response") }
      let response = try JSONDecoder().decode(IntentResponse.self, from: data)
      if let error = response.error { throw LoomIntentError(message: error) }
      waiter.continuation.resume(returning: response)
    } catch { waiter.continuation.resume(throwing: error) }
  }
  static func request(_ request: IntentRequest) async throws -> IntentResponse {
    try Task.checkCancellation()
    guard let submit else { throw LoomIntentError(message: "loomtty is not ready") }
    // Bound before JSON encoding, which may expand control characters.
    if let input = request.input, input.utf8.count > 256 * 1024 {
      throw LoomIntentError(message: "Input is limited to 256 KiB per command")
    }
    let data = try JSONEncoder().encode(request)
    guard data.count <= 2 * 1024 * 1024, next < UInt64.max else {
      throw LoomIntentError(message: "App Intent request is too large")
    }
    next += 1
    let id = next
    return try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation { continuation in
        guard !Task.isCancelled else {
          continuation.resume(throwing: CancellationError())
          return
        }
        let timeout = Task { @MainActor in
          do { try await Task.sleep(nanoseconds: 35_000_000_000) } catch { return }
          fail(
            id,
            LoomIntentError(
              message: "Terminal operation timed out; inspect the terminal before retrying"))
        }
        pending[id] = Pending(continuation: continuation, timeout: timeout)
        data.withUnsafeBytes { buffer in
          submit(
            id, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, loomIntentComplete)
        }
      }
    } onCancel: {
      Task { @MainActor in fail(id, CancellationError()) }
    }
  }
}

// C callbacks enter from Rust's AppKit/event-loop thread. assumeIsolated avoids
// dispatching a second main-thread task and makes reentrancy/lifetimes explicit.
private func loomIntentComplete(_ id: UInt64, _ bytes: UnsafePointer<UInt8>?, _ count: Int) {
  let data = bytes.flatMap { count >= 0 ? Data(bytes: $0, count: count) : nil }
  MainActor.assumeIsolated { IntentBridge.complete(id, data: data) }
}
@_cdecl("loom_app_intents_install")
func loomAppIntentsInstall(_ submit: HostSubmit, _ cancel: HostCancel) {
  MainActor.assumeIsolated { IntentBridge.install(submit: submit, cancel: cancel) }
}
@_cdecl("loom_app_intents_shutdown")
func loomAppIntentsShutdown() {
  MainActor.assumeIsolated { IntentBridge.shutdown() }
}
