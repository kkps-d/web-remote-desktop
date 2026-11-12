using System.Text.RegularExpressions;
using Gst;
using Gst.Sdp;
using Gst.WebRTC;
using DateTime = System.DateTime;

namespace Agent;

/// <summary>Thrown when the browser's offer can't be answered, e.g. it has no usable codec.</summary>
sealed class InvalidOfferException(string message) : Exception(message);

/// <summary>
/// One viewer's stream: a capture → encode → webrtcbin pipeline that answers the viewer's
/// SDP offer and runs until it is disposed or the connection ends.
/// </summary>
sealed class StreamSession : IDisposable
{
    static readonly TimeSpan SetupTimeout = TimeSpan.FromSeconds(10);
    // How long the viewer may take to connect after setup before the session ends.
    static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(30);
    // How long a "disconnected" connection may take to recover before the session ends.
    static readonly TimeSpan DisconnectGrace = TimeSpan.FromSeconds(10);

    readonly Pipeline pipeline;
    readonly Element webrtc;
    readonly ILogger logger;
    readonly Timer connectionMonitor;
    DateTime? connectDeadline;
    DateTime? disconnectedSince;
    string? endReason;
    int disposed;

    public string Id { get; } = Guid.NewGuid().ToString("N");
    public string AnswerSdp { get; private set; } = "";

    /// <summary>Why the session ended, or null while it runs. Set before <see cref="Ended"/> is raised.</summary>
    public string? EndReason => Volatile.Read(ref endReason);

    /// <summary>Raised once, from a background thread, when the connection fails or the pipeline errors.</summary>
    public event Action<StreamSession>? Ended;

    StreamSession(H264Codec codec, ILogger logger)
    {
        this.logger = logger;
        pipeline = (Pipeline)Parse.Launch(PipelineDescription(codec));
        webrtc = pipeline.GetByName("webrtc");
        connectionMonitor = new Timer(_ => CheckConnection());
    }

    /// <summary>Starts streaming and answers the offer. Blocks for up to a few seconds.</summary>
    public static StreamSession Start(string offerSdp, H264Codec codec, ILogger logger)
    {
        var session = new StreamSession(codec, logger);
        try
        {
            new Thread(session.WatchBus) { IsBackground = true, Name = $"bus {session.Id}" }.Start();
            session.pipeline.SetState(State.Playing);
            session.AnswerSdp = session.Answer(offerSdp);
            session.connectDeadline = DateTime.UtcNow + ConnectTimeout;
            session.connectionMonitor.Change(TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(1));
            return session;
        }
        catch
        {
            session.Dispose();
            throw;
        }
    }

    // webrtcbin matches our stream to the offer by intersecting RTP caps exactly, so the payload
    // type and profile-level-id must be the browser's. Desktop resolutions need a higher H.264
    // level than browsers offer; they accept it (level-asymmetry-allowed=1), so capssetter just
    // relabels the caps.
    static string PipelineDescription(H264Codec codec) =>
        "d3d11screencapturesrc show-cursor=true ! d3d11convert ! " +
        "video/x-raw(memory:D3D11Memory),format=NV12 ! " +
        "mfh264enc low-latency=true bitrate=8000 gop-size=60 ! h264parse ! " +
        "video/x-h264,profile=constrained-baseline ! " +
        $"rtph264pay pt={codec.PayloadType} config-interval=-1 aggregate-mode=zero-latency ! " +
        $"capssetter caps=\"application/x-rtp,profile-level-id=(string){codec.ProfileLevelId}\" ! " +
        "webrtcbin name=webrtc bundle-policy=max-bundle";

    public record H264Codec(int PayloadType, string ProfileLevelId);

    /// <summary>Finds the browser's constrained-baseline, packetization-mode=1 H.264 codec in the offer.</summary>
    public static H264Codec FindH264Codec(string offerSdp)
    {
        foreach (Match rtpmap in Regex.Matches(offerSdp, @"a=rtpmap:(\d+) H264/90000"))
        {
            var payloadType = rtpmap.Groups[1].Value;
            var fmtp = Regex.Match(offerSdp, $@"a=fmtp:{payloadType} (.*)").Groups[1].Value;
            var profileLevelId = Regex.Match(fmtp, "profile-level-id=(42e0[0-9a-f]{2})").Groups[1].Value;
            if (fmtp.Contains("packetization-mode=1") && profileLevelId != "")
                return new H264Codec(int.Parse(payloadType), profileLevelId);
        }
        throw new InvalidOfferException("The offer has no constrained-baseline H.264 codec.");
    }

    string Answer(string offerSdp)
    {
        // Until the first frame arrives our video pad has no caps, and webrtcbin can't match it
        // to the offer; it would answer with an unrelated, inactive transceiver.
        var videoPad = webrtc.GetStaticPad("sink_0");
        WaitUntil(() => videoPad.CurrentCaps != null, "No video from the capture pipeline.");

        if (SDPMessage.NewFromText(offerSdp, out var sdp) != SDPResult.Ok)
            throw new InvalidOfferException("The offer is not valid SDP.");
        EmitAndWait("set-remote-description", WebRTCSessionDescription.New(WebRTCSDPType.Offer, sdp));

        // The bindings can't marshal a null argument, so pass an empty options structure.
        var reply = EmitAndWait("create-answer", new Structure("options"));
        var answer = (WebRTCSessionDescription)reply!.GetValue("answer");
        EmitAndWait("set-local-description", answer);

        // Non-trickle ICE: wait until every local candidate is in the local description.
        WaitUntil(
            () => Convert.ToInt32(webrtc["ice-gathering-state"]) == (int)WebRTCICEGatheringState.Complete,
            "ICE gathering did not complete.");

        return Native.GetLocalDescriptionSdp(webrtc);
    }

    // webrtcbin's action signals take a GstPromise as their last argument and resolve it when done.
    Structure? EmitAndWait(string signal, object argument)
    {
        var promise = new Promise();
        // Interrupting the promise makes Wait return, so a signal that never replies can't hang setup.
        using (new Timer(_ => promise.Interrupt(), null, SetupTimeout, Timeout.InfiniteTimeSpan))
        {
            webrtc.Emit(signal, argument, promise);
            if (promise.Wait() != PromiseResult.Replied) throw new TimeoutException($"webrtcbin {signal} did not reply.");
        }

        // webrtcbin reports failures as an "error" field in the reply. Every signal here negotiates
        // the viewer's offer, so a failure means the offer can't be answered.
        var reply = promise.RetrieveReply();
        if (reply?.HasField("error") == true)
            throw new InvalidOfferException($"The offer was rejected: {Native.GetErrorMessage(reply) ?? $"{signal} failed"}");
        return reply;
    }

    void WaitUntil(Func<bool> condition, string timeoutMessage)
    {
        var deadline = DateTime.UtcNow + SetupTimeout;
        while (!condition())
        {
            // Fail with the pipeline's own error instead of waiting out the timeout.
            if (EndReason is { } reason) throw new InvalidOperationException(reason);
            if (DateTime.UtcNow > deadline) throw new TimeoutException(timeoutMessage);
            Thread.Sleep(10);
        }
    }

    void CheckConnection()
    {
        var state = (WebRTCPeerConnectionState)Convert.ToInt32(webrtc["connection-state"]);
        if (state is WebRTCPeerConnectionState.Connected)
            connectDeadline = null;
        if (state is WebRTCPeerConnectionState.Disconnected)
            disconnectedSince ??= DateTime.UtcNow;
        else
            disconnectedSince = null;

        var neverConnected = connectDeadline is { } deadline && DateTime.UtcNow > deadline;
        var lostTooLong = disconnectedSince is { } since && DateTime.UtcNow - since > DisconnectGrace;
        if (state is WebRTCPeerConnectionState.Failed or WebRTCPeerConnectionState.Closed || lostTooLong || neverConnected)
        {
            var reason = neverConnected ? $"Viewer did not connect (connection {state})" : $"Connection {state}";
            logger.LogInformation("Session {Id} ended: {Reason}", Id, reason);
            End(reason);
        }
    }

    void WatchBus()
    {
        var bus = pipeline.Bus;
        while (Volatile.Read(ref disposed) == 0)
        {
            var message = bus.TimedPopFiltered(200 * Gst.Constants.MSECOND, MessageType.Error | MessageType.Warning);
            if (message == null) continue;

            // The message structure holds both the error text and the debug details.
            if (message.Type == MessageType.Warning)
            {
                logger.LogWarning("Session {Id} pipeline warning from {Source}: {Details}", Id, message.Src?.Name, message.Structure);
                continue;
            }
            logger.LogError("Session {Id} pipeline error from {Source}: {Details}", Id, message.Src?.Name, message.Structure);
            End($"Pipeline error from {message.Src?.Name}: {message.Structure}");
            return;
        }
    }

    // The session can end during setup, before anyone subscribes to Ended. Recording the reason
    // first lets setup and ViewerSessions see it either way.
    void End(string reason)
    {
        if (Interlocked.CompareExchange(ref endReason, reason, null) != null) return;
        if (Volatile.Read(ref disposed) == 0) Ended?.Invoke(this);
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref disposed, 1) == 1) return;
        connectionMonitor.Dispose();
        pipeline.SetState(State.Null);
        pipeline.Dispose();
    }
}
