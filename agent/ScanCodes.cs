namespace Agent;

/// <summary>
/// Maps <c>KeyboardEvent.code</c> (a physical key) to its scan code, so the remote keyboard
/// layout decides which character a key types.
/// </summary>
static class ScanCodes
{
    /// <summary>A key as SendInput takes it: a scan code, or a virtual key where scan codes misbehave.</summary>
    public readonly record struct Key(ushort ScanCode, bool Extended, ushort VirtualKey = 0);

    public static bool TryGet(string code, out Key key) => Keys.TryGetValue(code, out key);

    // PS/2 set 1 scan codes; E0-prefixed keys are marked extended.
    internal static readonly Dictionary<string, Key> Keys = new()
    {
        ["Escape"] = Basic(0x01),
        ["Digit1"] = Basic(0x02), ["Digit2"] = Basic(0x03), ["Digit3"] = Basic(0x04), ["Digit4"] = Basic(0x05),
        ["Digit5"] = Basic(0x06), ["Digit6"] = Basic(0x07), ["Digit7"] = Basic(0x08), ["Digit8"] = Basic(0x09),
        ["Digit9"] = Basic(0x0A), ["Digit0"] = Basic(0x0B),
        ["Minus"] = Basic(0x0C), ["Equal"] = Basic(0x0D), ["Backspace"] = Basic(0x0E), ["Tab"] = Basic(0x0F),
        ["KeyQ"] = Basic(0x10), ["KeyW"] = Basic(0x11), ["KeyE"] = Basic(0x12), ["KeyR"] = Basic(0x13),
        ["KeyT"] = Basic(0x14), ["KeyY"] = Basic(0x15), ["KeyU"] = Basic(0x16), ["KeyI"] = Basic(0x17),
        ["KeyO"] = Basic(0x18), ["KeyP"] = Basic(0x19), ["BracketLeft"] = Basic(0x1A), ["BracketRight"] = Basic(0x1B),
        ["Enter"] = Basic(0x1C), ["ControlLeft"] = Basic(0x1D),
        ["KeyA"] = Basic(0x1E), ["KeyS"] = Basic(0x1F), ["KeyD"] = Basic(0x20), ["KeyF"] = Basic(0x21),
        ["KeyG"] = Basic(0x22), ["KeyH"] = Basic(0x23), ["KeyJ"] = Basic(0x24), ["KeyK"] = Basic(0x25),
        ["KeyL"] = Basic(0x26), ["Semicolon"] = Basic(0x27), ["Quote"] = Basic(0x28), ["Backquote"] = Basic(0x29),
        ["ShiftLeft"] = Basic(0x2A), ["Backslash"] = Basic(0x2B),
        ["KeyZ"] = Basic(0x2C), ["KeyX"] = Basic(0x2D), ["KeyC"] = Basic(0x2E), ["KeyV"] = Basic(0x2F),
        ["KeyB"] = Basic(0x30), ["KeyN"] = Basic(0x31), ["KeyM"] = Basic(0x32), ["Comma"] = Basic(0x33),
        ["Period"] = Basic(0x34), ["Slash"] = Basic(0x35), ["ShiftRight"] = Basic(0x36),
        ["NumpadMultiply"] = Basic(0x37), ["AltLeft"] = Basic(0x38), ["Space"] = Basic(0x39), ["CapsLock"] = Basic(0x3A),
        ["F1"] = Basic(0x3B), ["F2"] = Basic(0x3C), ["F3"] = Basic(0x3D), ["F4"] = Basic(0x3E), ["F5"] = Basic(0x3F),
        ["F6"] = Basic(0x40), ["F7"] = Basic(0x41), ["F8"] = Basic(0x42), ["F9"] = Basic(0x43), ["F10"] = Basic(0x44),
        ["ScrollLock"] = Basic(0x46),
        ["Numpad7"] = Basic(0x47), ["Numpad8"] = Basic(0x48), ["Numpad9"] = Basic(0x49), ["NumpadSubtract"] = Basic(0x4A),
        ["Numpad4"] = Basic(0x4B), ["Numpad5"] = Basic(0x4C), ["Numpad6"] = Basic(0x4D), ["NumpadAdd"] = Basic(0x4E),
        ["Numpad1"] = Basic(0x4F), ["Numpad2"] = Basic(0x50), ["Numpad3"] = Basic(0x51), ["Numpad0"] = Basic(0x52),
        ["NumpadDecimal"] = Basic(0x53), ["IntlBackslash"] = Basic(0x56), ["F11"] = Basic(0x57), ["F12"] = Basic(0x58),
        ["F13"] = Basic(0x64), ["F14"] = Basic(0x65), ["F15"] = Basic(0x66), ["F16"] = Basic(0x67), ["F17"] = Basic(0x68),
        ["F18"] = Basic(0x69), ["F19"] = Basic(0x6A), ["F20"] = Basic(0x6B), ["F21"] = Basic(0x6C), ["F22"] = Basic(0x6D),
        ["F23"] = Basic(0x6E), ["F24"] = Basic(0x76),
        ["KanaMode"] = Basic(0x70), ["IntlRo"] = Basic(0x73), ["Convert"] = Basic(0x79), ["NonConvert"] = Basic(0x7B),
        ["IntlYen"] = Basic(0x7D), ["NumpadComma"] = Basic(0x7E),

        ["MediaTrackPrevious"] = Extended(0x10), ["MediaTrackNext"] = Extended(0x19), ["NumpadEnter"] = Extended(0x1C),
        ["ControlRight"] = Extended(0x1D), ["AudioVolumeMute"] = Extended(0x20), ["MediaPlayPause"] = Extended(0x22),
        ["MediaStop"] = Extended(0x24), ["AudioVolumeDown"] = Extended(0x2E), ["AudioVolumeUp"] = Extended(0x30),
        ["NumpadDivide"] = Extended(0x35), ["PrintScreen"] = Extended(0x37), ["AltRight"] = Extended(0x38),
        // Windows reads Num Lock as E0 45; plain 45 is Pause.
        ["NumLock"] = Extended(0x45),
        ["Home"] = Extended(0x47), ["ArrowUp"] = Extended(0x48), ["PageUp"] = Extended(0x49), ["ArrowLeft"] = Extended(0x4B),
        ["ArrowRight"] = Extended(0x4D), ["End"] = Extended(0x4F), ["ArrowDown"] = Extended(0x50), ["PageDown"] = Extended(0x51),
        ["Insert"] = Extended(0x52), ["Delete"] = Extended(0x53), ["MetaLeft"] = Extended(0x5B), ["MetaRight"] = Extended(0x5C),
        ["ContextMenu"] = Extended(0x5D),

        // Pause's real scan sequence (E1 1D 45) can't be sent as one scan code, so use VK_PAUSE.
        ["Pause"] = new(0, false, VirtualKey: 0x13),
    };

    static Key Basic(ushort scanCode) => new(scanCode, false);

    static Key Extended(ushort scanCode) => new(scanCode, true);
}
