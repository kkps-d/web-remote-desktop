using System.Text.RegularExpressions;
using Gst;

namespace Agent;

/// <summary>An encoder the viewer can pick, and why it can't be used if it isn't available.</summary>
record EncoderOption(string Id, string Label, bool Available, string? UnavailableReason);

/// <summary>The frame sizes an encoder accepts, from its sink pad template.</summary>
record SizeLimits(int MinWidth, int MaxWidth, int MinHeight, int MaxHeight)
{
    public bool Fits(Resolution size) =>
        size.Width >= MinWidth && size.Width <= MaxWidth && size.Height >= MinHeight && size.Height <= MaxHeight;

    /// <summary>The size scaled down, keeping its aspect ratio, until it is no larger than the maximum.</summary>
    public Resolution ShrinkToFit(Resolution size)
    {
        var scale = Math.Min(1, Math.Min((double)MaxWidth / size.Width, (double)MaxHeight / size.Height));
        // The encoders need even dimensions.
        return new Resolution((int)(size.Width * scale) / 2 * 2, (int)(size.Height * scale) / 2 * 2);
    }

    public override string ToString() => $"{MinWidth}×{MinHeight} to {MaxWidth}×{MaxHeight}";
}

/// <summary>
/// The H.264 encoders from docs/ENCODERS.md and which of them work on this machine. GPU encoders
/// count only on the GPU that drives the captured display; any other GPU would need a copy
/// between GPUs for every frame.
/// </summary>
sealed class EncoderCatalog
{
    // Describe builds the encoder segment that sits between the capture's D3D11 NV12 output and
    // h264parse, from the factory name and bitrate. The element named "encoder" takes bitrate
    // changes. Keyframes every 60 frames let a viewer recover quickly from packet loss.
    record Definition(string Id, string Label, string FactoryPattern, bool OnGpu, Func<string, int, string> Describe);

    // In priority order: the first available one is the default.
    static readonly Definition[] Definitions =
    [
        new("nvenc", "NVIDIA NVENC", @"^nvd3d11h264(device\d+)?enc$", true, (factory, kbps) =>
            $"{factory} name=encoder tune=ultra-low-latency bframes=0 rc-lookahead=0 gop-size=60 bitrate={kbps}"),
        new("amf", "AMD AMF", @"^amfh264(device\d+)?enc$", true, (factory, kbps) =>
            $"{factory} name=encoder usage=ultra-low-latency gop-size=60 bitrate={kbps}"),
        new("mf", "Media Foundation", @"^mfh264(device\d+)?enc$", true, (factory, kbps) =>
            $"{factory} name=encoder low-latency=true gop-size=60 bitrate={kbps}"),
        // d3d12upload takes the D3D11 frames without a trip through system memory.
        new("d3d12", "Direct3D 12", @"^d3d12h264(device\d+)?enc$", true, (factory, kbps) =>
            $"d3d12upload ! {factory} name=encoder rate-control=cbr gop-size=60 bitrate={kbps}"),
        new("x264", "x264 (CPU)", "^x264enc$", false, (factory, kbps) =>
            $"d3d11download ! {factory} name=encoder tune=zerolatency speed-preset=ultrafast key-int-max=60 bitrate={kbps}"),
        // Microsoft's software encoder registers as an mfh264*enc without the Hardware class.
        new("mf-software", "Microsoft (CPU)", @"^mfh264(device\d+)?enc$", false, (factory, kbps) =>
            $"d3d11download ! {factory} name=encoder low-latency=true gop-size=60 bitrate={kbps}"),
    ];

    record Available(string Factory, SizeLimits Limits);

    readonly Dictionary<string, Available> available = new();

    public IReadOnlyList<EncoderOption> Options { get; }

    /// <summary>The highest-priority available encoder.</summary>
    public string Default { get; }

    public EncoderCatalog(ILogger<EncoderCatalog> logger)
    {
        var display = FindDisplayAdapter();
        var installed = Registry.Get().GetFeatureList(ElementFactory.GType).OfType<ElementFactory>().ToList();

        var options = new List<EncoderOption>();
        foreach (var definition in Definitions)
        {
            var candidates = installed
                .Where(factory => Regex.IsMatch(factory.Name, definition.FactoryPattern) && IsHardware(factory) == definition.OnGpu)
                // The unsuffixed factory (the first adapter) before the deviceN ones.
                .OrderBy(factory => factory.Name.Length).ThenBy(factory => factory.Name)
                .ToList();
            var match = definition.OnGpu
                ? candidates.FirstOrDefault(factory => display is not null && RunsOn(factory, display))
                : candidates.FirstOrDefault();

            // Once NVENC has run, running any Media Foundation encoder (hardware or software)
            // makes every later NVENC session in this process fail with
            // NV_ENC_ERR_INVALID_VERSION, so they're only offered when NVENC isn't.
            if (definition.Id.StartsWith("mf") && available.ContainsKey("nvenc"))
            {
                options.Add(new(definition.Id, definition.Label, false, "Off because it breaks NVENC until the agent restarts"));
                continue;
            }

            string? reason = null;
            if (match is not null)
                available[definition.Id] = new(match.Name, ReadSizeLimits(match));
            else if (!candidates.Any())
                reason = "Not installed";
            else
                reason = display is null ? "Couldn't find the display's GPU" : $"Not on this display's GPU ({display.Description})";
            options.Add(new(definition.Id, definition.Label, match is not null, reason));
        }

        Options = options;
        Default = options.FirstOrDefault(option => option.Available)?.Id
            ?? throw new InvalidOperationException("No H.264 encoder is available. Install GStreamer with the \"Complete\" option.");
        logger.LogInformation("Display GPU: {Gpu}. Encoders: {Encoders}", display?.Description ?? "unknown",
            string.Join(", ", available.Select(pair => $"{pair.Key} ({pair.Value.Factory}, {pair.Value.Limits})")));
    }

    public bool IsAvailable(string id) => available.ContainsKey(id);

    public string Label(string id) => Definitions.Single(definition => definition.Id == id).Label;

    /// <summary>The frame sizes an available encoder accepts.</summary>
    public SizeLimits Limits(string id) => available[id].Limits;

    /// <summary>The encoder segment for <see cref="Parse.BinFromDescription"/>.</summary>
    public string Describe(string id, int bitrateKbps) =>
        Definitions.Single(definition => definition.Id == id).Describe(available[id].Factory, bitrateKbps);

    record Adapter(long Luid, string Description);

    // d3d11screencapturesrc captures the primary monitor by default; its device properties name
    // the adapter that drives it.
    static Adapter? FindDisplayAdapter()
    {
        var monitor = new DeviceMonitor();
        monitor.AddFilter("Source/Monitor", null);
        if (!monitor.Start()) return null;
        try
        {
            foreach (var device in monitor.Devices)
            {
                var properties = device.Properties;
                properties.GetBoolean("device.primary", out var primary);
                if (properties.GetString("device.api") != "d3d11" || !primary) continue;
                properties.GetInt64("device.adapter.luid", out var luid);
                return new Adapter(luid, properties.GetString("device.adapter.description") ?? "unknown GPU");
            }
            return null;
        }
        finally
        {
            monitor.Stop();
        }
    }

    // Read from the factory rather than an instance; see RunsOn. Hardware encoders report their
    // GPU's limits here. Every sink structure (D3D11 or system memory) has the same ranges.
    static SizeLimits ReadSizeLimits(ElementFactory factory)
    {
        var template = factory.StaticPadTemplates.First(template => template.Direction == PadDirection.Sink);
        var structure = template.StaticCaps.Get().GetStructure(0);
        var (minWidth, maxWidth) = Range(structure, "width");
        var (minHeight, maxHeight) = Range(structure, "height");
        return new SizeLimits(minWidth, maxWidth, minHeight, maxHeight);

        static (int Min, int Max) Range(Structure structure, string field) =>
            structure.GetValue(field).Val is IntRange range ? (range.Min, range.Max) : (1, int.MaxValue);
    }

    static bool IsHardware(ElementFactory factory) =>
        factory.GetMetadata("klass")?.Contains("Hardware") == true;

    // GPU encoders register one factory per adapter; an instance's adapter-luid names it.
    // Creating an NVENC element here counts as running NVENC, and the Media Foundation
    // instances created after it would then break NVENC (see the constructor), so NVENC is
    // matched by vendor instead. With several NVIDIA GPUs that picks the first one's encoder.
    static bool RunsOn(ElementFactory factory, Adapter display)
    {
        if (factory.Name.StartsWith("nvd3d11")) return display.Description.Contains("NVIDIA");
        using var element = ElementFactory.Make(factory.Name);
        return element?["adapter-luid"] is long luid && luid == display.Luid;
    }
}
