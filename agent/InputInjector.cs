using System.Runtime.InteropServices;

namespace Agent;

/// <summary>
/// Injects mouse and keyboard input on the primary display with SendInput, and remembers what
/// is held so it can be released when the viewer goes away. Windows-only.
/// </summary>
sealed class InputInjector
{
    const uint InputMouse = 0, InputKeyboard = 1;
    const uint MouseMove = 0x0001, MouseAbsolute = 0x8000, MouseWheel = 0x0800, MouseHWheel = 0x1000;
    const uint KeyUp = 0x0002, KeyScanCode = 0x0008, KeyExtended = 0x0001;
    const int ScreenWidth = 0, ScreenHeight = 1;

    // DOM button numbers (0 left, 1 middle, 2 right) to MOUSEEVENTF down/up flags.
    static readonly (uint Down, uint Up)[] ButtonFlags = [(0x0002, 0x0004), (0x0020, 0x0040), (0x0008, 0x0010)];

    readonly Lock gate = new();
    readonly HashSet<int> heldButtons = [];
    readonly HashSet<string> heldKeys = [];

    /// <summary>Moves to a position given as 0–1 fractions of the primary display.</summary>
    public void MoveTo(double x, double y)
    {
        var (width, height) = DisplaySize();
        MoveToPixel((int)(Math.Clamp(x, 0, 1) * width), (int)(Math.Clamp(y, 0, 1) * height), width, height);
    }

    /// <summary>
    /// Moves by whole pixels. Relative SendInput moves go through Windows pointer acceleration,
    /// so the delta is applied to the cursor position and sent as an absolute move instead.
    /// </summary>
    public void MoveBy(int dx, int dy)
    {
        var (width, height) = DisplaySize();
        GetCursorPos(out var cursor);
        MoveToPixel(cursor.X + dx, cursor.Y + dy, width, height);
    }

    /// <summary>Scrolls by wheel units (120 per notch), positive meaning right and down like DOM deltas.</summary>
    public void Scroll(int dx, int dy)
    {
        // Windows' vertical wheel is positive away from the user, i.e. up.
        if (dy != 0) SendMouse(new MouseInput { MouseData = -dy, Flags = MouseWheel });
        if (dx != 0) SendMouse(new MouseInput { MouseData = dx, Flags = MouseHWheel });
    }

    public void Button(int button, bool down)
    {
        if (button < 0 || button >= ButtonFlags.Length) return;
        lock (gate)
        {
            if (down) heldButtons.Add(button);
            else if (!heldButtons.Remove(button)) return;
            SendMouse(new MouseInput { Flags = down ? ButtonFlags[button].Down : ButtonFlags[button].Up });
        }
    }

    /// <summary>Presses or releases a key by its <c>KeyboardEvent.code</c>. Unknown codes are ignored.</summary>
    public void Key(string code, bool down)
    {
        if (!ScanCodes.TryGet(code, out var key)) return;
        lock (gate)
        {
            // A repeated down is an auto-repeat and is sent again.
            if (down) heldKeys.Add(code);
            else if (!heldKeys.Remove(code)) return;
            SendKey(key, down);
        }
    }

    /// <summary>Releases every held key and button, e.g. when the viewer disconnects.</summary>
    public void ReleaseAll()
    {
        lock (gate)
        {
            foreach (var code in heldKeys)
                if (ScanCodes.TryGet(code, out var key)) SendKey(key, false);
            foreach (var button in heldButtons)
                SendMouse(new MouseInput { Flags = ButtonFlags[button].Up });
            heldKeys.Clear();
            heldButtons.Clear();
        }
    }

    // The process is per-monitor DPI aware (see Program), so this is the primary display in physical pixels.
    static (int Width, int Height) DisplaySize() => (GetSystemMetrics(ScreenWidth), GetSystemMetrics(ScreenHeight));

    // Absolute coordinates span 0–65535 over the primary display; aiming at the pixel's center
    // keeps Windows' truncation from landing on the pixel before it.
    static void MoveToPixel(int x, int y, int width, int height)
    {
        x = Math.Clamp(x, 0, width - 1);
        y = Math.Clamp(y, 0, height - 1);
        SendMouse(new MouseInput
        {
            Dx = (int)((x + 0.5) * 65536 / width),
            Dy = (int)((y + 0.5) * 65536 / height),
            Flags = MouseMove | MouseAbsolute,
        });
    }

    static void SendMouse(MouseInput mouse) => Send(new Input { Type = InputMouse, Data = new InputData { Mouse = mouse } });

    static void SendKey(ScanCodes.Key key, bool down)
    {
        var keyboard = key.VirtualKey != 0
            ? new KeyboardInput { VirtualKey = key.VirtualKey }
            : new KeyboardInput { ScanCode = key.ScanCode, Flags = KeyScanCode | (key.Extended ? KeyExtended : 0) };
        if (!down) keyboard.Flags |= KeyUp;
        Send(new Input { Type = InputKeyboard, Data = new InputData { Keyboard = keyboard } });
    }

    static void Send(Input input)
    {
        // Fails when the foreground window belongs to a more privileged process (UIPI); nothing to do then.
        SendInput(1, [input], Marshal.SizeOf<Input>());
    }

    [StructLayout(LayoutKind.Sequential)]
    struct MouseInput
    {
        public int Dx, Dy, MouseData;
        public uint Flags, Time;
        public IntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct KeyboardInput
    {
        public ushort VirtualKey, ScanCode;
        public uint Flags, Time;
        public IntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    struct InputData
    {
        [FieldOffset(0)] public MouseInput Mouse;
        [FieldOffset(0)] public KeyboardInput Keyboard;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct Input
    {
        public uint Type;
        public InputData Data;
    }

    struct Point
    {
        public int X, Y;
    }

    [DllImport("user32.dll")]
    static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll")]
    static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll")]
    static extern bool SetProcessDpiAwarenessContext(IntPtr context);

    /// <summary>
    /// Makes the process per-monitor DPI aware, so cursor positions and screen sizes are in
    /// physical pixels like the captured display. Call once at startup.
    /// </summary>
    public static void UsePhysicalPixels() => SetProcessDpiAwarenessContext(-4); // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
}
