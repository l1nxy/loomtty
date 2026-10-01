import AppIntents
import Foundation

@available(macOS 13.0, *)
struct TerminalEntity: AppEntity {
  let id: String
  @Property(title: "Title") var title: String
  @Property(title: "Working Directory") var directory: String
  @Property(title: "Session") var session: String
  static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Terminal")
  static let defaultQuery = TerminalQuery()
  var displayRepresentation: DisplayRepresentation {
    DisplayRepresentation(title: "\(title.isEmpty ? session : title)", subtitle: "\(directory)")
  }
  init(_ record: TerminalRecord) {
    id = record.id
    title = record.title
    directory = record.directory
    session = record.session
  }
}

@available(macOS 13.0, *)
struct TerminalQuery: EntityStringQuery {
  @MainActor
  func entities(for identifiers: [String]) async throws -> [TerminalEntity] {
    let all = try await IntentBridge.request(IntentRequest(action: "list")).terminals
    // Preserve requested order and omit expired IDs. Never substitute the
    // focused terminal for a saved entity from an earlier application run.
    let byID = Dictionary(uniqueKeysWithValues: all.map { ($0.id, $0) })
    return identifiers.compactMap { byID[$0].map(TerminalEntity.init) }
  }
  @MainActor
  func suggestedEntities() async throws -> [TerminalEntity] {
    try await IntentBridge.request(IntentRequest(action: "list")).terminals.map(TerminalEntity.init)
  }
  @MainActor
  func entities(matching string: String) async throws -> [TerminalEntity] {
    let all = try await suggestedEntities()
    if string.isEmpty { return all }
    return all.filter {
      $0.title.localizedCaseInsensitiveContains(string)
        || $0.session.localizedCaseInsensitiveContains(string)
        || $0.directory.localizedCaseInsensitiveContains(string)
    }
  }
}

@available(macOS 13.0, *)
enum TerminalLocation: String, AppEnum {
  case window, tab, right, down
  static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Terminal Location")
  static let caseDisplayRepresentations: [Self: DisplayRepresentation] = [
    .window: "New Window", .tab: "New Tab", .right: "Split Right", .down: "Split Down",
  ]
}

@available(macOS 13.0, *)
struct NewTerminalIntent: AppIntent {
  // Rust activates only the explicit window/focus action. Keep entity queries
  // and input from requesting a separate foreground launch or Dock reopen.
  static let openAppWhenRun = false
  #if compiler(>=6.2)
    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .background }
  #endif
  static let authenticationPolicy: IntentAuthenticationPolicy = .requiresLocalDeviceAuthentication
  static let title: LocalizedStringResource = "New Terminal"
  static let description = IntentDescription(
    "Create a terminal window, native tab, or split. Returns the connected terminal for the next action."
  )
  @Parameter(title: "Location", default: .window) var location: TerminalLocation
  @Parameter(title: "Parent Terminal", description: "Optional for a tab; required for a split.")
  var parent: TerminalEntity?
  @Parameter(
    title: "Working Directory",
    description: "An existing absolute folder path, for new windows and tabs.") var directory:
    String?
  static var parameterSummary: some ParameterSummary {
    Summary("Create terminal in \(\.$location)") {
      \.$parent
      \.$directory
    }
  }
  @MainActor
  func perform() async throws -> some IntentResult & ReturnsValue<TerminalEntity> {
    let action: String
    switch location {
    case .window: action = "new_window"
    case .tab: action = "new_tab"
    case .right, .down:
      guard parent != nil else {
        throw LoomIntentError(message: "Choose a parent terminal for the split")
      }
      guard directory == nil || directory == "" else {
        throw LoomIntentError(
          message:
            "Splits inherit their terminal's directory; choose a new window or tab to set a directory"
        )
      }
      action = "split"
    }
    let response = try await IntentBridge.request(
      IntentRequest(
        action: action, target: location == .window ? nil : parent?.id,
        directory: directory.flatMap { $0.isEmpty ? nil : $0 }, direction: location.rawValue
      ))
    guard let terminal = response.terminal else {
      throw LoomIntentError(message: "The new terminal is unavailable")
    }
    return .result(value: TerminalEntity(terminal))
  }
}

@available(macOS 13.0, *)
struct FindTerminalsIntent: AppIntent {
  // Rust activates only the explicit window/focus action. Keep entity queries
  // and input from requesting a separate foreground launch or Dock reopen.
  static let openAppWhenRun = false
  #if compiler(>=6.2)
    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .background }
  #endif
  static let authenticationPolicy: IntentAuthenticationPolicy = .requiresLocalDeviceAuthentication
  static let title: LocalizedStringResource = "Find Terminals"
  static let description = IntentDescription(
    "Find connected terminals by title, session, or working directory. Empty text returns all normal-window terminals."
  )
  @Parameter(title: "Search Text", default: "") var text: String
  static var parameterSummary: some ParameterSummary {
    Summary("Find terminals matching \(\.$text)")
  }
  @MainActor
  func perform() async throws -> some IntentResult & ReturnsValue<[TerminalEntity]> {
    return .result(value: try await TerminalQuery().entities(matching: text))
  }
}

@available(macOS 13.0, *)
struct FocusTerminalIntent: AppIntent {
  // Rust activates only the explicit window/focus action. Keep entity queries
  // and input from requesting a separate foreground launch or Dock reopen.
  static let openAppWhenRun = false
  #if compiler(>=6.2)
    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .background }
  #endif
  static let authenticationPolicy: IntentAuthenticationPolicy = .requiresLocalDeviceAuthentication
  static let title: LocalizedStringResource = "Focus Terminal"
  @Parameter(title: "Terminal") var terminal: TerminalEntity
  static var parameterSummary: some ParameterSummary { Summary("Focus \(\.$terminal)") }
  @MainActor
  func perform() async throws -> some IntentResult {
    _ = try await IntentBridge.request(IntentRequest(action: "focus", target: terminal.id))
    return .result()
  }
}

@available(macOS 13.0, *)
struct InputTextIntent: AppIntent {
  // Rust activates only the explicit window/focus action. Keep entity queries
  // and input from requesting a separate foreground launch or Dock reopen.
  static let openAppWhenRun = false
  #if compiler(>=6.2)
    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .background }
  #endif
  static let authenticationPolicy: IntentAuthenticationPolicy = .requiresLocalDeviceAuthentication
  static let title: LocalizedStringResource = "Send Text to Terminal"
  static let description = IntentDescription(
    "Send exact text to a terminal. No Return key or newline is added. A newline in the text can execute a shell command."
  )
  @Parameter(title: "Terminal") var terminal: TerminalEntity
  @Parameter(
    title: "Text",
    inputOptions: String.IntentInputOptions(
      capitalizationType: .none, multiline: true, autocorrect: false, smartQuotes: false,
      smartDashes: false
    )) var text: String
  static var parameterSummary: some ParameterSummary {
    Summary("Send \(\.$text) to \(\.$terminal)")
  }
  @MainActor
  func perform() async throws -> some IntentResult {
    _ = try await IntentBridge.request(
      IntentRequest(action: "input", target: terminal.id, input: text))
    return .result()
  }
}

@available(macOS 13.0, *)
struct CloseTerminalIntent: AppIntent {
  // Rust activates only the explicit window/focus action. Keep entity queries
  // and input from requesting a separate foreground launch or Dock reopen.
  static let openAppWhenRun = false
  #if compiler(>=6.2)
    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .background }
  #endif
  static let authenticationPolicy: IntentAuthenticationPolicy = .requiresLocalDeviceAuthentication
  static let title: LocalizedStringResource = "Close Terminal"
  static let description = IntentDescription("Close a terminal pane and end its process.")
  @Parameter(title: "Terminal") var terminal: TerminalEntity
  static var parameterSummary: some ParameterSummary { Summary("Close \(\.$terminal)") }
  @MainActor
  func perform() async throws -> some IntentResult {
    _ = try await IntentBridge.request(IntentRequest(action: "close", target: terminal.id))
    return .result()
  }
}

@available(macOS 13.0, *)
struct LoomShortcuts: AppShortcutsProvider {
  static var appShortcuts: [AppShortcut] {
    AppShortcut(
      intent: NewTerminalIntent(),
      phrases: [
        "Open a terminal in \(.applicationName)"
      ], shortTitle: "New Terminal", systemImageName: "terminal")
  }
}
