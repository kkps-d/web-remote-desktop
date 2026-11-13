using System.Diagnostics;
using Gst;

namespace Agent;

record EncodingStats(double TotalEncodeTimeMs, long FramesEncoded);

/// <summary>Measures elapsed time from encoder input to output for matching frame timestamps.</summary>
sealed class EncodeTiming
{
    readonly Lock gate = new();
    readonly Dictionary<ulong, long> pending = new();
    double totalMilliseconds;
    long framesEncoded;

    /// <summary>
    /// Times the given encoder bin, including any upload or download it does around the encoder.
    /// Totals carry over from a previous encoder so clients' per-interval deltas stay valid.
    /// </summary>
    public void Attach(Bin encoderBin)
    {
        lock (gate) pending.Clear();
        encoderBin.GetStaticPad("sink").AddProbe(PadProbeType.Buffer, OnInput);
        encoderBin.GetStaticPad("src").AddProbe(PadProbeType.Buffer, OnOutput);
    }

    PadProbeReturn OnInput(Pad pad, PadProbeInfo info)
    {
        var pts = RunningTime(pad, info);
        if (pts == ulong.MaxValue) return PadProbeReturn.Ok;
        lock (gate)
        {
            // Dropped input frames have no output; bound their retained timestamps.
            if (pending.Count >= 240) pending.Clear();
            pending[pts] = Stopwatch.GetTimestamp();
        }
        return PadProbeReturn.Ok;
    }

    PadProbeReturn OnOutput(Pad pad, PadProbeInfo info)
    {
        var pts = RunningTime(pad, info);
        lock (gate)
        {
            if (pending.Remove(pts, out var started))
            {
                totalMilliseconds += Stopwatch.GetElapsedTime(started).TotalMilliseconds;
                framesEncoded++;
            }
        }
        return PadProbeReturn.Ok;
    }

    public EncodingStats Snapshot()
    {
        lock (gate) return new(totalMilliseconds, framesEncoded);
    }

    static ulong RunningTime(Pad pad, PadProbeInfo info)
    {
        // GstVideoEncoder rebases output PTS. Segment running time matches both sides.
        using var segmentEvent = pad.GetStickyEvent(EventType.Segment, 0);
        if (segmentEvent is null) return ulong.MaxValue;
        using var buffer = info.Buffer;
        return Native.GetRunningTime(segmentEvent, buffer.Pts);
    }
}
