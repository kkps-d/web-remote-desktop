using System.Runtime.InteropServices;

namespace Agent;

/// <summary>
/// Injects touch contacts on the primary display through a synthetic touch device, so Windows
/// handles them as real touch (pinch-zoom, inertia scrolling, press-and-hold). Each frame
/// carries every contact on the screen. Needs Windows 10 1809 or later.
/// </summary>
sealed class TouchInjector : IDisposable
{
    public const int MaxContacts = 10;
    internal const uint TouchPointer = 2; // PT_TOUCH
    internal const uint FlagInRange = 0x2, FlagInContact = 0x4, FlagCanceled = 0x8000;
    internal const uint FlagDown = 0x10000, FlagUpdate = 0x20000, FlagUp = 0x40000;
    internal const uint MaskContactArea = 0x1; // TOUCH_MASK_CONTACTAREA
    internal const int ContactRadius = 4;
    const uint FeedbackDefault = 1; // POINTER_FEEDBACK_DEFAULT: Windows draws its touch feedback

    // Windows cancels a contact that gets no updates for a while, so held contacts are resent.
    static readonly TimeSpan KeepAliveInterval = TimeSpan.FromMilliseconds(100);

    readonly Lock gate = new();
    // Viewer contact id to its Windows pointer id (0 to MaxContacts - 1) and last pixel.
    readonly Dictionary<int, (uint PointerId, int X, int Y)> held = [];
    readonly Action<PointerTypeInfo[]> send;
    readonly Func<(int Width, int Height)> displaySize;
    readonly Timer keepAliveTimer;
    readonly TimeSpan keepAliveInterval;
    readonly bool useDevice;
    IntPtr device;
    bool deviceFailed;

    /// <summary>
    /// Injects with a synthetic touch device, or hands frames to <paramref name="send"/> instead
    /// (for tests, which also pass the display size and may turn the keep-alive timer off).
    /// </summary>
    public TouchInjector(
        Action<PointerTypeInfo[]>? send = null,
        Func<(int Width, int Height)>? displaySize = null,
        TimeSpan? keepAliveInterval = null)
    {
        useDevice = send is null;
        this.send = send ?? SendToDevice;
        this.displaySize = displaySize ?? InputInjector.DisplaySize;
        this.keepAliveInterval = keepAliveInterval ?? KeepAliveInterval;
        keepAliveTimer = new Timer(_ => KeepAlive());
    }

    /// <summary>
    /// Applies the contacts now on the screen, as 0–1 fractions of the display: new ids go down,
    /// known ids move, missing ids lift, or are canceled if <paramref name="canceled"/> is set.
    /// Contacts past <see cref="MaxContacts"/> are ignored.
    /// </summary>
    public void Update(IReadOnlyList<(int Id, double X, double Y)> contacts, bool canceled = false)
    {
        lock (gate)
        {
            if (contacts.Count == 0 && held.Count == 0) return;
            if (useDevice && !EnsureDevice()) return;
            var (width, height) = displaySize();
            var frame = new List<PointerTypeInfo>();
            var present = new HashSet<int>();

            foreach (var (id, x, y) in contacts)
            {
                if (!present.Add(id)) continue;
                var pixel = InputInjector.ToPixel(x, y, width, height);
                if (held.TryGetValue(id, out var contact))
                {
                    held[id] = (contact.PointerId, pixel.X, pixel.Y);
                    frame.Add(Info(contact.PointerId, pixel.X, pixel.Y, FlagUpdate | FlagInRange | FlagInContact));
                }
                else if (FreePointerId() is { } pointerId)
                {
                    held[id] = (pointerId, pixel.X, pixel.Y);
                    frame.Add(Info(pointerId, pixel.X, pixel.Y, FlagDown | FlagInRange | FlagInContact));
                }
            }

            // Lifted after the loop, so a pointer id is never reused within the frame that lifts it.
            foreach (var id in held.Keys.Where(id => !present.Contains(id)).ToList())
            {
                var (pointerId, x, y) = held[id];
                held.Remove(id);
                frame.Add(Info(pointerId, x, y, canceled ? FlagUp | FlagCanceled : FlagUp));
            }

            if (frame.Count > 0) send([.. frame]);
            ScheduleKeepAlive();
        }
    }

    /// <summary>Cancels every held contact, e.g. when the viewer disconnects.</summary>
    public void CancelAll() => Update([], canceled: true);

    /// <summary>Resends held contacts where they are; the timer calls this.</summary>
    internal void KeepAlive()
    {
        lock (gate)
        {
            if (held.Count == 0) return;
            send([.. held.Values.Select(c => Info(c.PointerId, c.X, c.Y, FlagUpdate | FlagInRange | FlagInContact))]);
        }
    }

    public void Dispose()
    {
        CancelAll();
        lock (gate)
        {
            keepAliveTimer.Dispose();
            if (device != IntPtr.Zero) DestroySyntheticPointerDevice(device);
            device = IntPtr.Zero;
            deviceFailed = true;
        }
    }

    // Restarts the countdown on every frame, so it only fires while contacts are held still.
    void ScheduleKeepAlive()
    {
        var interval = held.Count > 0 ? keepAliveInterval : Timeout.InfiniteTimeSpan;
        keepAliveTimer.Change(interval, interval);
    }

    uint? FreePointerId()
    {
        for (uint pointerId = 0; pointerId < MaxContacts; pointerId++)
            if (!held.Values.Any(contact => contact.PointerId == pointerId)) return pointerId;
        return null;
    }

    // Created on the first touch, so sessions without touch don't get a device.
    bool EnsureDevice()
    {
        if (device != IntPtr.Zero) return true;
        if (deviceFailed) return false;
        device = CreateSyntheticPointerDevice(TouchPointer, MaxContacts, FeedbackDefault);
        if (device != IntPtr.Zero) return true;
        deviceFailed = true;
        throw new InvalidOperationException(
            $"Couldn't create a touch device (error {Marshal.GetLastPInvokeError()}); touch input is ignored from now on.");
    }

    // Windows accepts contacts without a contact area but never delivers them to windows.
    static PointerTypeInfo Info(uint pointerId, int x, int y, uint flags) => new()
    {
        Type = TouchPointer,
        Touch = new PointerTouchInfo
        {
            PointerInfo = new PointerInfo
            {
                PointerType = TouchPointer,
                PointerId = pointerId,
                PointerFlags = flags,
                PixelLocation = new Point { X = x, Y = y },
            },
            TouchMask = MaskContactArea,
            Contact = new Rect { Left = x - ContactRadius, Top = y - ContactRadius, Right = x + ContactRadius, Bottom = y + ContactRadius },
        },
    };

    void SendToDevice(PointerTypeInfo[] frame)
    {
        // Fails when the foreground window belongs to a more privileged process (UIPI), like SendInput.
        InjectSyntheticPointerInput(device, frame, (uint)frame.Length);
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Point
    {
        public int X, Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Rect
    {
        public int Left, Top, Right, Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PointerInfo
    {
        public uint PointerType, PointerId, FrameId, PointerFlags;
        public IntPtr SourceDevice, HwndTarget;
        public Point PixelLocation, HimetricLocation, PixelLocationRaw, HimetricLocationRaw;
        public uint Time, HistoryCount;
        public int InputData;
        public uint KeyStates;
        public ulong PerformanceCount;
        public int ButtonChangeType;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PointerTouchInfo
    {
        public PointerInfo PointerInfo;
        public uint TouchFlags, TouchMask;
        public Rect Contact, ContactRaw;
        public uint Orientation, Pressure;
    }

    // POINTER_TYPE_INFO: the union's other member, POINTER_PEN_INFO, is smaller than touch.
    [StructLayout(LayoutKind.Sequential)]
    internal struct PointerTypeInfo
    {
        public uint Type;
        public PointerTouchInfo Touch;
    }

    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr CreateSyntheticPointerDevice(uint pointerType, uint maxCount, uint feedbackMode);

    [DllImport("user32.dll", SetLastError = true)]
    static extern bool InjectSyntheticPointerInput(IntPtr device, PointerTypeInfo[] pointerInfo, uint count);

    [DllImport("user32.dll")]
    static extern void DestroySyntheticPointerDevice(IntPtr device);
}
