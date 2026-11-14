using System.Runtime.InteropServices;
using System.Text.Json;

namespace Agent;

/// <summary>
/// Receives the viewer's "input" data channel from webrtcbin and injects its messages. Held
/// keys and buttons are released when the channel closes or the session ends.
/// </summary>
sealed class InputChannel : IDisposable
{
    // The bindings have no GstWebRTCDataChannel wrapper, so signals are connected directly.
    delegate void DataChannelHandler(IntPtr webrtc, IntPtr channel, IntPtr userData);
    delegate void ChannelHandler(IntPtr channel, IntPtr userData);
    delegate void StringHandler(IntPtr channel, IntPtr text, IntPtr userData);

    readonly InputInjector injector = new();
    readonly Action<bool> setStreamCursor;
    readonly string sessionId;
    readonly ILogger logger;
    // GStreamer calls these from its own threads; the fields keep the delegates alive.
    readonly DataChannelHandler onDataChannel;
    readonly ChannelHandler onOpen;
    readonly ChannelHandler onClose;
    readonly StringHandler onMessage;

    /// <summary>Must be created before the offer is set, which announces the channel.</summary>
    public InputChannel(Gst.Element webrtc, string sessionId, Action<bool> setStreamCursor, ILogger logger)
    {
        this.sessionId = sessionId;
        this.setStreamCursor = setStreamCursor;
        this.logger = logger;
        onDataChannel = OnDataChannel;
        onOpen = (_, _) => logger.LogInformation("Session {Id}: input channel open", sessionId);
        onClose = (_, _) => Closed("closed");
        onMessage = OnMessage;
        g_signal_connect_data(webrtc.Handle, "on-data-channel", onDataChannel, IntPtr.Zero, IntPtr.Zero, 0);
    }

    /// <summary>Releases held input after the channel failed, e.g. its SCTP association ended.</summary>
    public void Closed(string reason)
    {
        logger.LogInformation("Session {Id}: input channel {Reason}", sessionId, reason);
        injector.ReleaseAll();
    }

    void OnDataChannel(IntPtr webrtc, IntPtr newChannel, IntPtr userData)
    {
        g_object_get(newChannel, "label", out var labelPtr, IntPtr.Zero);
        var label = Marshal.PtrToStringUTF8(labelPtr);
        g_free(labelPtr);
        if (label != "input")
        {
            logger.LogWarning("Session {Id}: ignoring data channel \"{Label}\"", sessionId, label);
            return;
        }

        // webrtcbin owns the channel until the pipeline is freed, so no ref is taken.
        g_signal_connect_data(newChannel, "on-open", onOpen, IntPtr.Zero, IntPtr.Zero, 0);
        g_signal_connect_data(newChannel, "on-close", onClose, IntPtr.Zero, IntPtr.Zero, 0);
        g_signal_connect_data(newChannel, "on-message-string", onMessage, IntPtr.Zero, IntPtr.Zero, 0);
    }

    void OnMessage(IntPtr source, IntPtr textPtr, IntPtr userData)
    {
        var text = Marshal.PtrToStringUTF8(textPtr) ?? "";
        try
        {
            using var json = JsonDocument.Parse(text);
            var message = json.RootElement;
            switch (message.GetProperty("type").GetString())
            {
                case "move":
                    injector.MoveTo(message.GetProperty("x").GetDouble(), message.GetProperty("y").GetDouble());
                    break;
                case "moveBy":
                    injector.MoveBy(message.GetProperty("dx").GetInt32(), message.GetProperty("dy").GetInt32());
                    break;
                case "button":
                    injector.Button(message.GetProperty("button").GetInt32(), message.GetProperty("down").GetBoolean());
                    break;
                case "wheel":
                    injector.Scroll(message.GetProperty("dx").GetInt32(), message.GetProperty("dy").GetInt32());
                    break;
                case "key":
                    injector.Key(message.GetProperty("code").GetString()!, message.GetProperty("down").GetBoolean());
                    break;
                case "releaseAll":
                    injector.ReleaseAll();
                    break;
                case "cursor":
                    setStreamCursor(message.GetProperty("visible").GetBoolean());
                    break;
                default:
                    logger.LogWarning("Session {Id}: unknown input message {Text}", sessionId, text);
                    break;
            }
        }
        catch (Exception ex) when (ex is JsonException or KeyNotFoundException or InvalidOperationException or FormatException)
        {
            logger.LogWarning("Session {Id}: invalid input message {Text}: {Error}", sessionId, text, ex.Message);
        }
    }

    /// <summary>Call after the pipeline has stopped, so no more messages arrive.</summary>
    public void Dispose() => injector.ReleaseAll();

    [DllImport("gobject-2.0-0.dll")]
    static extern ulong g_signal_connect_data(IntPtr instance, string signal, Delegate handler, IntPtr data, IntPtr destroy, int flags);

    [DllImport("gobject-2.0-0.dll")]
    static extern void g_object_get(IntPtr obj, string property, out IntPtr value, IntPtr terminator);

    [DllImport("glib-2.0-0.dll")]
    static extern void g_free(IntPtr memory);
}
