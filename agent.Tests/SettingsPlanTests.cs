using Agent;

namespace Agent.Tests;

public class SettingsPlanTests
{
    static readonly Resolution Source = new(5120, 2880);

    // NVENC tops out at 4096 px; x264 takes any size.
    static readonly Dictionary<string, SizeLimits> Encoders = new()
    {
        ["nvenc"] = new(145, 4096, 49, 4096),
        ["x264"] = new(16, 16384, 16, 16384),
        ["large-only"] = new(3000, 8192, 1600, 8192),
    };

    static readonly StreamSettings Current = new(4096, 2304, 30, 8000, "nvenc");

    static StreamSession.SettingsPlan Plan(StreamSettingsUpdate update, StreamSettings? current = null) =>
        StreamSession.PlanUpdate(current ?? Current, update, Source,
            id => Encoders.TryGetValue(id, out var limits) ? limits : null, id => $"Label {id}");

    static StreamSettingsUpdate Change(int? width = null, int? height = null, int? framerate = null,
        int? bitrateKbps = null, string? encoder = null) => new(width, height, framerate, bitrateKbps, encoder);

    [Fact]
    public void OmittedFieldsKeepTheirValue() =>
        Assert.Equal(Current with { Framerate = 60 }, Plan(Change(framerate: 60)).Next);

    [Fact]
    public void AnEmptyUpdateChangesNothing() =>
        Assert.Equal(new StreamSession.SettingsPlan(Current, false), Plan(Change()));

    [Theory]
    [InlineData(1920, null)]
    [InlineData(null, 1080)]
    public void WidthAndHeightMustBeSetTogether(int? width, int? height) =>
        Assert.Throws<InvalidSettingsException>(() => Plan(Change(width, height)));

    [Theory]
    [InlineData(1921, 1080)] // odd
    [InlineData(1920, 1081)]
    [InlineData(62, 1080)] // below 64 px
    [InlineData(5122, 2880)] // larger than the source
    [InlineData(5120, 2882)]
    public void RejectsResolutionsTheStreamCantUse(int width, int height) =>
        Assert.Throws<InvalidSettingsException>(() => Plan(Change(width, height, encoder: "x264")));

    [Theory]
    [InlineData(0, 8000)]
    [InlineData(121, 8000)]
    [InlineData(30, 499)]
    [InlineData(30, 50_001)]
    public void RejectsFrameRatesAndBitratesOutOfRange(int framerate, int bitrateKbps) =>
        Assert.Throws<InvalidSettingsException>(() => Plan(Change(framerate: framerate, bitrateKbps: bitrateKbps)));

    [Theory]
    [InlineData(1, 500)]
    [InlineData(120, 50_000)]
    public void AcceptsTheEndsOfEachRange(int framerate, int bitrateKbps) =>
        Assert.Equal(Current with { Framerate = framerate, BitrateKbps = bitrateKbps },
            Plan(Change(framerate: framerate, bitrateKbps: bitrateKbps)).Next);

    [Fact]
    public void RejectsAnUnavailableEncoder() =>
        Assert.Throws<InvalidSettingsException>(() => Plan(Change(encoder: "amf")));

    [Fact]
    public void RejectsASizeOutsideTheEncodersLimitsNamingIt()
    {
        var error = Assert.Throws<InvalidSettingsException>(() => Plan(Change(5120, 2880)));
        Assert.Equal("Label nvenc supports 145×49 to 4096×4096.", error.Message);
    }

    [Fact]
    public void SwitchesFirstWhenTheNewEncoderTakesTheCurrentSize()
    {
        // NVENC can't take 5120 px, so x264 must be running before the resize.
        var plan = Plan(Change(5120, 2880, encoder: "x264"));
        Assert.Equal(new StreamSettings(5120, 2880, 30, 8000, "x264"), plan.Next);
        Assert.True(plan.SwitchEncoderFirst);
    }

    [Fact]
    public void ResizesFirstWhenOnlyTheCurrentEncoderTakesBothSizes()
    {
        // NVENC can't take x264's current 5120 px, so shrink before switching.
        var plan = Plan(Change(4096, 2304, encoder: "nvenc"), Current with { Width = 5120, Height = 2880, Encoder = "x264" });
        Assert.False(plan.SwitchEncoderFirst);
    }

    [Fact]
    public void RejectsAChangeNoOrderCanApply()
    {
        // Neither NVENC at 4200 px wide nor "large-only" at 1920 px works.
        var current = Current with { Width = 1920, Height = 1080 };
        var error = Assert.Throws<InvalidSettingsException>(() => Plan(Change(4200, 2400, encoder: "large-only"), current));
        Assert.Equal("Change the resolution and the encoder separately.", error.Message);
    }
}
