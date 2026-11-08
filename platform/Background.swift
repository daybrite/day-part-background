import Foundation
import BackgroundTasks

private enum DayBackground {
    static var events: UInt64 = 0
    static var counter: Int64 = 0
    static var running: [Int64: BGTask] = [:]
    static var registered = Set<String>()
    static let defaults = UserDefaults.standard
    static func key(_ id: String) -> String { "day.background.schedule." + id }
    static func emit(_ object: [String: Any]) {
        guard events != 0, let data = try? JSONSerialization.data(withJSONObject: object),
              let text = String(data: data, encoding: .utf8) else { return }
        events_native_emit(events, text)
    }
    static func submit(_ request: [String: Any], retry: Bool = false) throws {
        guard let id = request["identifier"] as? String, registered.contains(id) else {
            throw NSError(domain: "DayBackground", code: 1, userInfo: [NSLocalizedDescriptionKey: "Task identifier is not registered or permitted in Info.plist"])
        }
        let task = BGProcessingTaskRequest(identifier: id)
        task.requiresNetworkConnectivity = request["requires_network"] as? Bool ?? false
        task.requiresExternalPower = request["requires_charging"] as? Bool ?? false
        let delay = request["delay_secs"] as? Double ?? 0
        task.earliestBeginDate = Date(timeIntervalSinceNow: retry ? max(delay, 900) : delay)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: id)
        try BGTaskScheduler.shared.submit(task)
    }
    static func execute(_ task: BGTask) {
        guard let request = defaults.dictionary(forKey: key(task.identifier)) else {
            task.setTaskCompleted(success: false); return
        }
        counter += 1
        let run = counter
        running[run] = task
        task.expirationHandler = {
            DispatchQueue.main.async {
                guard let expired = running.removeValue(forKey: run) else { return }
                emit(["event": "cancel", "run": run])
                expired.setTaskCompleted(success: false)
            }
        }
        // Renew repeating work before execution; expiry must not silently erase the schedule.
        if let interval = request["interval_secs"] as? Double {
            var next = request; next["delay_secs"] = interval
            do { try submit(next) } catch { NSLog("Day background rescheduling failed: %@", String(describing: error)) }
        }
        emit(["event": "run", "run": run, "identifier": task.identifier, "payload": request["payload"] as? String ?? ""])
    }
}

func events_native(events: UInt64) throws { DayBackground.events = events }
func initialize_native(identifiers: String) throws {
    guard Thread.isMainThread else {
        throw NSError(domain: "DayBackground", code: 2, userInfo: [NSLocalizedDescriptionKey: "Register background tasks during application launch on the main thread"])
    }
    guard let data = identifiers.data(using: .utf8), let ids = try JSONSerialization.jsonObject(with: data) as? [String] else { return }
    let permitted = Bundle.main.object(forInfoDictionaryKey: "BGTaskSchedulerPermittedIdentifiers") as? [String] ?? []
    for id in ids where permitted.contains(id) {
        if BGTaskScheduler.shared.register(forTaskWithIdentifier: id, using: .main, launchHandler: DayBackground.execute) {
            DayBackground.registered.insert(id)
        }
    }
}
func schedule_native(request: String, done: UInt64) throws {
    DispatchQueue.main.async {
        do {
            guard let data = request.data(using: .utf8), var object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let id = object["identifier"] as? String else { throw NSError(domain: "DayBackground", code: 3) }
            // Property lists cannot store JSON null values.
            if object["interval_secs"] is NSNull { object.removeValue(forKey: "interval_secs") }
            try DayBackground.submit(object)
            DayBackground.defaults.set(object, forKey: DayBackground.key(id))
            schedule_native_complete(done, "")
        } catch { schedule_native_complete(done, String(describing: error)) }
    }
}
func cancel_native(identifier: String, done: UInt64) throws {
    DispatchQueue.main.async {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier)
        DayBackground.defaults.removeObject(forKey: DayBackground.key(identifier))
        let active = DayBackground.running.filter { $0.value.identifier == identifier }
        for (run, task) in active {
            DayBackground.emit(["event": "cancel", "run": run])
            DayBackground.running.removeValue(forKey: run)
            task.setTaskCompleted(success: false)
        }
        cancel_native_complete(done, "")
    }
}
func finish_native(run: Int64, result: String) throws {
    DispatchQueue.main.async {
        guard let task = DayBackground.running.removeValue(forKey: run) else { return }
        task.expirationHandler = nil
        if result == "retry", let request = DayBackground.defaults.dictionary(forKey: DayBackground.key(task.identifier)) {
            do { try DayBackground.submit(request, retry: true) }
            catch { NSLog("Day background retry failed: %@", String(describing: error)) }
        }
        task.setTaskCompleted(success: result == "success")
    }
}
