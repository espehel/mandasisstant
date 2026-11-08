import Foundation

/// Namespace for the AppleBridge core API.
///
/// Translation and the microphone permission request land here in later phases. This module
/// must stay free of FFI concerns: plain Swift types, async/await, throwing APIs.
public enum AppleBridge {
    public static let version = "0.1.0"
}
