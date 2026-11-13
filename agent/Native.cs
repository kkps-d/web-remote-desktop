using System.Runtime.InteropServices;

namespace Agent;

/// <summary>Direct native calls for things the gstreamer-sharp bindings get wrong or lack.</summary>
static class Native
{
    const int EnumCurrentSettings = -1;

    // DEVMODEW, display variant.
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct DevMode
    {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
        public ushort SpecVersion, DriverVersion, Size, DriverExtra;
        public uint Fields;
        public int PositionX, PositionY;
        public uint DisplayOrientation, DisplayFixedOutput;
        public short Color, Duplex, YResolution, TTOption, Collate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string FormName;
        public ushort LogPixels;
        public uint BitsPerPel, PelsWidth, PelsHeight, DisplayFlags, DisplayFrequency;
        public uint IcmMethod, IcmIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern bool EnumDisplaySettingsW(string? deviceName, int modeNum, ref DevMode devMode);

    /// <summary>
    /// The primary display's mode in physical pixels, which d3d11screencapturesrc captures by
    /// default; unlike GetSystemMetrics, it isn't scaled for DPI. Null if it can't be read.
    /// </summary>
    public static Resolution? GetPrimaryDisplaySize()
    {
        var mode = new DevMode { Size = (ushort)Marshal.SizeOf<DevMode>() };
        return EnumDisplaySettingsW(null, EnumCurrentSettings, ref mode)
            ? new Resolution((int)mode.PelsWidth, (int)mode.PelsHeight)
            : null;
    }

    [DllImport("gstreamer-1.0-0.dll")]
    static extern void gst_event_parse_segment(IntPtr ev, out IntPtr segment);

    [DllImport("gstreamer-1.0-0.dll")]
    static extern ulong gst_segment_to_running_time(IntPtr segment, Gst.Format format, ulong position);

    // The old bindings marshal GstSegment with the wrong layout for current GStreamer.
    public static ulong GetRunningTime(Gst.Event segmentEvent, ulong pts)
    {
        if (pts == ulong.MaxValue) return ulong.MaxValue;
        gst_event_parse_segment(segmentEvent.Handle, out var segment);
        return gst_segment_to_running_time(segment, Gst.Format.Time, pts);
    }

    [DllImport("gobject-2.0-0.dll")]
    static extern void g_object_get(IntPtr obj, string property, out IntPtr value, IntPtr terminator);

    [DllImport("gstsdp-1.0-0.dll")]
    static extern IntPtr gst_sdp_message_as_text(IntPtr sdp);

    [DllImport("gstwebrtc-1.0-0.dll")]
    static extern void gst_webrtc_session_description_free(IntPtr description);

    [DllImport("glib-2.0-0.dll")]
    static extern void g_free(IntPtr memory);

    [DllImport("gstreamer-1.0-0.dll")]
    static extern IntPtr gst_structure_get_value(IntPtr structure, string field);

    [DllImport("gobject-2.0-0.dll")]
    static extern IntPtr g_value_get_boxed(IntPtr value);

    /// <summary>
    /// Returns the message of the GError in a promise reply's "error" field, or null if there is
    /// none. The bindings can't unbox a GError, so read it directly.
    /// </summary>
    public static string? GetErrorMessage(Gst.Structure reply)
    {
        var value = gst_structure_get_value(reply.Handle, "error");
        var error = value == IntPtr.Zero ? IntPtr.Zero : g_value_get_boxed(value);
        if (error == IntPtr.Zero) return null;

        // struct GError { GQuark domain; gint code; gchar *message; }
        return Marshal.PtrToStringUTF8(Marshal.ReadIntPtr(error, 8));
    }

    /// <summary>
    /// Returns webrtcbin's local description as SDP text. Reading the property through the
    /// bindings frees the boxed copy too early and the SDP pointer dangles; g_object_get hands
    /// us our own copy, which we free when done.
    /// </summary>
    public static string GetLocalDescriptionSdp(Gst.Element webrtc)
    {
        g_object_get(webrtc.Handle, "local-description", out IntPtr description, IntPtr.Zero);
        try
        {
            // struct GstWebRTCSessionDescription { GstWebRTCSDPType type; GstSDPMessage *sdp; }
            var sdp = Marshal.ReadIntPtr(description, IntPtr.Size);
            var text = gst_sdp_message_as_text(sdp);
            try { return Marshal.PtrToStringUTF8(text)!; }
            finally { g_free(text); }
        }
        finally
        {
            gst_webrtc_session_description_free(description);
        }
    }
}
