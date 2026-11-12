using System.Runtime.InteropServices;

namespace Agent;

/// <summary>Direct native calls for things the gstreamer-sharp bindings get wrong.</summary>
static class Native
{
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
