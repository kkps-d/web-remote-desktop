using System.Text.RegularExpressions;
using Gst;
using Gst.Sdp;
using Gst.WebRTC;
using DateTime = System.DateTime;

namespace Agent;

/// <summary>Thrown when the browser's offer can't be answered, e.g. it has no usable codec.</summary>
sealed class InvalidOfferException(string message) : Exception(message);

/// <summary>Thrown when requested stream settings are out of range.</summary>
sealed class InvalidSettingsException(string message) : Exception(message);

record Resolution(int Width, int Height);

record StreamSettings(int Width, int Height, int Framerate, int BitrateKbps, string Encoder);

/// <summary>A partial settings change; null fields keep their current value.</summary>
record StreamSettingsUpdate(int? Width, int? Height, int? Framerate, int? BitrateKbps, string? Encoder);

/// <summary>What the client sees about a session's stream.</summary>
record StreamInfo(Resolution Source, StreamSettings Settings, IReadOnlyList<EncoderOption> Encoders);

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

    const int DefaultFramerate = 30;
    const int MaxFramerate = 120;
    const int DefaultBitrateKbps = 8000;
    const int MinBitrateKbps = 500;
    const int MaxBitrateKbps = 50_000;
    const int MinDimension = 64;

    // Each wrapper holds a native ref (see Native), so the session keeps the ones it uses for its
    // whole life and Dispose releases them all. Probe callbacks reuse these pad wrappers too.
    readonly Pipeline pipeline;
    readonly Element webrtc;
    readonly Pad captureSrc;
    readonly Pad outputSrc;
    readonly Pad parseSink;
    readonly EncodeTiming encodeTiming = new();
    readonly InputChannel input;
    readonly EncoderCatalog encoders;
    readonly ILogger logger;
    EncoderBin encoderBin;
    readonly Timer connectionMonitor;
    // Keeps CheckConnection from reading webrtc while Dispose releases it.
    readonly Lock monitorGate = new();
    DateTime? connectDeadline;
    DateTime? disconnectedSince;
    string? endReason;
    int disposed;

    public string Id { get; } = Guid.NewGuid().ToString("N");
    public string AnswerSdp { get; private set; } = "";
    public EncodingStats EncodingStats => encodeTiming.Snapshot();

    /// <summary>Why the session ended, or null while it runs. Set before <see cref="Ended"/> is raised.</summary>
    public string? EndReason => Volatile.Read(ref endReason);

    /// <summary>Raised once, from a background thread, when the connection fails or the pipeline errors.</summary>
    public event Action<StreamSession>? Ended;

    StreamSession(H264Codec codec, EncoderCatalog encoders, ILogger logger)
    {
        this.encoders = encoders;
        this.logger = logger;
        Settings = new(0, 0, DefaultFramerate, DefaultBitrateKbps, encoders.Default);
        pipeline = Native.Sink((Pipeline)Parse.Launch(PipelineDescription(codec, InitialSize(encoders.Limits(Settings.Encoder)))));
        webrtc = Native.GetByName(pipeline, "webrtc");
        captureSrc = StaticPad("capture", "src");
        outputSrc = StaticPad("output", "src");
        parseSink = StaticPad("parse", "sink");
        encoderBin = CreateEncoderBin(Settings.Encoder, Settings.BitrateKbps);
        pipeline.Add(encoderBin.Bin);
        if (!LinkEncoder(encoderBin)) throw new InvalidOperationException($"Could not link the {Settings.Encoder} encoder.");
        encodeTiming.Attach(encoderBin.Sink, encoderBin.Src);
        connectionMonitor = new Timer(_ => CheckConnection());
        input = new InputChannel(webrtc, Id, SetStreamCursor, logger);
    }

    Pad StaticPad(string element, string pad)
    {
        using var owner = Native.GetByName(pipeline, element);
        return Native.GetStaticPad(owner, pad);
    }

    // The viewer hides the stream cursor while it shows its own (absolute pointer mode).
    void SetStreamCursor(bool visible)
    {
        using var capture = Native.GetByName(pipeline, "capture");
        capture["show-cursor"] = visible;
    }

    /// <summary>Starts streaming and answers the offer. Blocks for up to a few seconds.</summary>
    public static StreamSession Start(string offerSdp, H264Codec codec, EncoderCatalog encoders, ILogger logger)
    {
        var session = new StreamSession(codec, encoders, logger);
        try
        {
            var bus = Native.GetBus(session.pipeline);
            new Thread(() => session.WatchBus(bus)) { IsBackground = true, Name = $"bus {session.Id}" }.Start();
            session.pipeline.SetState(State.Playing);
            session.AnswerSdp = session.Answer(offerSdp);
            // Answer waits for the first frame, so the output caps are known by now.
            var size = CapsSize(session.outputSrc);
            session.Settings = session.Settings with { Width = size.Width, Height = size.Height };
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
    // relabels the caps. AMF labels its constrained output plain "baseline", so accept both.
    // The encoder bin goes between the two chains; see CreateEncoderBin.
    static string PipelineDescription(H264Codec codec, Resolution? size) =>
        "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert ! " +
        $"capsfilter name=output caps={OutputCaps(size, DefaultFramerate)}  " +
        "h264parse name=parse ! video/x-h264,profile=(string){constrained-baseline,baseline} ! " +
        $"rtph264pay pt={codec.PayloadType} config-interval=-1 aggregate-mode=zero-latency ! " +
        $"capssetter caps=\"application/x-rtp,profile-level-id=(string){codec.ProfileLevelId}\" ! " +
        "webrtcbin name=webrtc bundle-policy=max-bundle";

    // Streams start at native resolution (null), unless the display is larger than the encoder
    // accepts; then they start scaled down to fit, e.g. NVENC's 4096 px on a 5K display.
    static Resolution? InitialSize(SizeLimits limits)
    {
        var display = Native.GetPrimaryDisplaySize();
        if (display is null || limits.Fits(display)) return null;
        return limits.ShrinkToFit(display);
    }

    /// <summary>An encoder bin and the wrappers the session holds into it.</summary>
    sealed record EncoderBin(Bin Bin, Pad Sink, Pad Src, Element Encoder) : IDisposable
    {
        public void Dispose()
        {
            Encoder.Dispose();
            Src.Dispose();
            Sink.Dispose();
            Bin.Dispose();
        }
    }

    EncoderBin CreateEncoderBin(string encoder, int bitrateKbps)
    {
        var bin = Native.Sink(Parse.BinFromDescription(encoders.Describe(encoder, bitrateKbps), true));
        return new(bin, Native.GetStaticPad(bin, "sink"), Native.GetStaticPad(bin, "src"), Native.GetByName(bin, "encoder"));
    }

    bool LinkEncoder(EncoderBin bin) =>
        outputSrc.Link(bin.Sink) == PadLinkReturn.Ok && bin.Src.Link(parseSink) == PadLinkReturn.Ok;

    // Swaps the encoder bin while streaming. Blocking the capsfilter's output holds the next frame
    // while the bins are exchanged; the new encoder starts with a keyframe, and rtph264pay sends
    // SPS/PPS with it, so the browser keeps decoding without renegotiating.
    void SwapEncoder(string encoder, int bitrateKbps)
    {
        var next = CreateEncoderBin(encoder, bitrateKbps);
        using var swapped = new ManualResetEventSlim();
        string? failure = null;

        outputSrc.AddProbe(PadProbeType.BlockDownstream, (_, _) =>
        {
            try
            {
                var previous = encoderBin;
                outputSrc.Unlink(previous.Sink);
                previous.Src.Unlink(parseSink);
                previous.Bin.SetState(State.Null);
                pipeline.Remove(previous.Bin);
                previous.Dispose();

                pipeline.Add(next.Bin);
                encoderBin = next;
                encodeTiming.Attach(next.Sink, next.Src);
                if (!LinkEncoder(next) || !next.Bin.SyncStateWithParent())
                    failure = $"Could not start the {encoder} encoder.";
            }
            catch (Exception ex)
            {
                failure = $"Could not switch to the {encoder} encoder: {ex.Message}";
            }
            finally
            {
                swapped.Set();
            }
            return PadProbeReturn.Remove;
        });

        // The probe fires on the next frame, which the capture source sends at the frame rate.
        if (!swapped.Wait(SetupTimeout)) failure = "The encoder switch timed out.";
        if (failure is null) return;
        // The old encoder is gone, so there's nothing to fall back to.
        logger.LogError("Session {Id}: {Failure}", Id, failure);
        End(failure);
        throw new InvalidOperationException(failure);
    }

    // d3d11convert scales to the output size, and the capture source captures at the output frame
    // rate. Changing these caps while playing renegotiates the pipeline; the encoder restarts with
    // a keyframe and browsers decode the new size without SDP renegotiation.
    static string OutputCaps(Resolution? size, int framerate) =>
        "video/x-raw(memory:D3D11Memory),format=NV12," +
        (size is null ? "" : $"width={size.Width},height={size.Height},") +
        $"framerate={framerate}/1";

    public record H264Codec(int PayloadType, string ProfileLevelId);

    /// <summary>The stream's current settings.</summary>
    public StreamSettings Settings { get; private set; }

    /// <summary>The captured desktop's resolution.</summary>
    public Resolution SourceResolution => CapsSize(captureSrc);

    static Resolution CapsSize(Pad pad)
    {
        using var caps = pad.CurrentCaps;
        var structure = caps.GetStructure(0);
        structure.GetInt("width", out var width);
        structure.GetInt("height", out var height);
        return new Resolution(width, height);
    }

    /// <summary>Applies the given settings while streaming. Null fields keep their current value.</summary>
    public void Update(StreamSettingsUpdate update)
    {
        if ((update.Width is null) != (update.Height is null))
            throw new InvalidSettingsException("Width and height must be set together.");

        var source = SourceResolution;
        var next = new StreamSettings(
            update.Width ?? Settings.Width,
            update.Height ?? Settings.Height,
            update.Framerate ?? Settings.Framerate,
            update.BitrateKbps ?? Settings.BitrateKbps,
            update.Encoder ?? Settings.Encoder);

        // The encoder needs even dimensions; upscaling would only cost bandwidth.
        if (next.Width < MinDimension || next.Height < MinDimension || next.Width > source.Width ||
            next.Height > source.Height || next.Width % 2 != 0 || next.Height % 2 != 0)
            throw new InvalidSettingsException(
                $"Resolution must be even, at least {MinDimension} px and no larger than {source.Width}×{source.Height}.");
        if (next.Framerate is < 1 or > MaxFramerate)
            throw new InvalidSettingsException($"Frame rate must be 1–{MaxFramerate} fps.");
        if (next.BitrateKbps is < MinBitrateKbps or > MaxBitrateKbps)
            throw new InvalidSettingsException($"Bitrate must be {MinBitrateKbps}–{MaxBitrateKbps} kbps.");
        if (!encoders.IsAvailable(next.Encoder))
            throw new InvalidSettingsException($"The {next.Encoder} encoder isn't available on this computer.");

        // An encoder given a size outside its limits fails and ends the session, so check first.
        var size = new Resolution(next.Width, next.Height);
        var currentSize = new Resolution(Settings.Width, Settings.Height);
        var limits = encoders.Limits(next.Encoder);
        if (!limits.Fits(size))
            throw new InvalidSettingsException($"{encoders.Label(next.Encoder)} supports {limits}.");
        if (next == Settings) return;

        // Every step must leave the running encoder with a size it accepts: switch encoder first
        // if the new one takes the current size, otherwise resize first.
        var encoderChanged = next.Encoder != Settings.Encoder;
        var switchFirst = encoderChanged && limits.Fits(currentSize);
        if (encoderChanged && !switchFirst && !encoders.Limits(Settings.Encoder).Fits(size))
            throw new InvalidSettingsException("Change the resolution and the encoder separately.");

        // A new encoder starts with the new bitrate. The others pick it up on their next frame.
        if (switchFirst)
            SwapEncoder(next.Encoder, next.BitrateKbps);
        if (size != currentSize || next.Framerate != Settings.Framerate)
        {
            using var output = Native.GetByName(pipeline, "output");
            using var caps = Caps.FromString(OutputCaps(size, next.Framerate));
            output["caps"] = caps;
        }
        if (encoderChanged && !switchFirst)
            SwapEncoder(next.Encoder, next.BitrateKbps);
        else if (!encoderChanged && next.BitrateKbps != Settings.BitrateKbps)
            encoderBin.Encoder["bitrate"] = (uint)next.BitrateKbps;

        Settings = next;
        logger.LogInformation("Session {Id} settings changed to {Settings}", Id, next);
    }

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
        using var videoPad = Native.GetStaticPad(webrtc, "sink_0");
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
        WebRTCPeerConnectionState state;
        lock (monitorGate)
        {
            if (Volatile.Read(ref disposed) == 1) return;
            state = (WebRTCPeerConnectionState)Convert.ToInt32(webrtc["connection-state"]);
        }
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

    // Takes ownership of the bus, so it stays valid after Dispose releases the pipeline.
    void WatchBus(Bus bus)
    {
        using var _ = bus;
        while (Volatile.Read(ref disposed) == 0)
        {
            // A message holds a ref to the element that posted it.
            using var message = bus.TimedPopFiltered(200 * Gst.Constants.MSECOND, MessageType.Error | MessageType.Warning);
            if (message == null) continue;

            // The message structure holds both the error text and the debug details.
            // SCTP carries the input channel; its association fails when the viewer closes the
            // connection. The video keeps going, and the connection monitor ends the session.
            if (message.Type == MessageType.Error && message.Src?.Name.StartsWith("sctp") == true)
            {
                input.Closed($"failed: {message.Src.Name} association ended");
                continue;
            }
            if (message.Type == MessageType.Warning)
            {
                // A frame rate change makes d3d11convert fail to renegotiate with the old input caps
                // until the capture source renegotiates upstream. It's expected and recovers at once.
                var debug = message.Structure.GetString("debug") ?? "";
                if (debug.Contains("gst_base_transform_reconfigure") && debug.Contains("not negotiated"))
                {
                    logger.LogDebug("Session {Id} renegotiating after a settings change", Id);
                    continue;
                }

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
        input.Dispose();

        // The pipeline and the D3D11 device it holds (with its GPU driver threads) are only
        // freed once every wrapper into it is released. Wait out a running connection check first.
        lock (monitorGate) webrtc.Dispose();
        encoderBin.Dispose();
        captureSrc.Dispose();
        outputSrc.Dispose();
        parseSink.Dispose();
        pipeline.Dispose();
    }
}
