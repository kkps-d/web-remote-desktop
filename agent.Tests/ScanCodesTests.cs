using Agent;

namespace Agent.Tests;

public class ScanCodesTests
{
    // KeyboardEvent.code values of a full-size US (104-key) and ISO (105-key) keyboard.
    public static TheoryData<string> StandardKeys() =>
    [
        "Escape", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        "PrintScreen", "ScrollLock", "Pause",
        "Backquote", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9",
        "Digit0", "Minus", "Equal", "Backspace",
        "Tab", "KeyQ", "KeyW", "KeyE", "KeyR", "KeyT", "KeyY", "KeyU", "KeyI", "KeyO", "KeyP",
        "BracketLeft", "BracketRight", "Backslash",
        "CapsLock", "KeyA", "KeyS", "KeyD", "KeyF", "KeyG", "KeyH", "KeyJ", "KeyK", "KeyL", "Semicolon", "Quote", "Enter",
        "ShiftLeft", "IntlBackslash", "KeyZ", "KeyX", "KeyC", "KeyV", "KeyB", "KeyN", "KeyM",
        "Comma", "Period", "Slash", "ShiftRight",
        "ControlLeft", "MetaLeft", "AltLeft", "Space", "AltRight", "MetaRight", "ContextMenu", "ControlRight",
        "Insert", "Home", "PageUp", "Delete", "End", "PageDown", "ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight",
        "NumLock", "NumpadDivide", "NumpadMultiply", "NumpadSubtract", "NumpadAdd", "NumpadEnter", "NumpadDecimal",
        "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
    ];

    [Theory]
    [MemberData(nameof(StandardKeys))]
    public void MapsEveryStandardKey(string code) => Assert.True(ScanCodes.TryGet(code, out _));

    [Fact]
    public void GivesEachCodeItsOwnKey()
    {
        var shared = ScanCodes.Keys.GroupBy(pair => pair.Value).Where(group => group.Count() > 1)
            .Select(group => string.Join(", ", group.Select(pair => pair.Key)));
        Assert.Empty(shared);
    }

    [Theory]
    [InlineData("KeyA", 0x1E, false)]
    [InlineData("Enter", 0x1C, false)]
    [InlineData("NumpadEnter", 0x1C, true)]
    [InlineData("ControlRight", 0x1D, true)]
    [InlineData("NumLock", 0x45, true)]
    [InlineData("F24", 0x76, false)]
    public void UsesSet1ScanCodes(string code, int scanCode, bool extended)
    {
        Assert.True(ScanCodes.TryGet(code, out var key));
        Assert.Equal(new ScanCodes.Key((ushort)scanCode, extended), key);
    }
}
