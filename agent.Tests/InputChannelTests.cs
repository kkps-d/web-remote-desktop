using Agent;
using Microsoft.Extensions.Logging;

namespace Agent.Tests;

public class InputChannelTests
{
    readonly List<InputInjector.Input> sent = [];
    readonly List<bool> cursor = [];
    readonly ListLogger logger = new();
    readonly List<TouchInjector.PointerTypeInfo[]> touchFrames = [];
    readonly InputInjector injector;
    readonly TouchInjector touch;

    public InputChannelTests()
    {
        injector = new InputInjector(sent.Add);
        touch = new TouchInjector(touchFrames.Add, () => (1920, 1080), Timeout.InfiniteTimeSpan);
    }

    void Receive(string text, Action<bool>? setStreamCursor = null) =>
        InputChannel.Receive(text, injector, touch, setStreamCursor ?? cursor.Add, logger, "session");

    IEnumerable<uint> TouchFlags(int frame) => touchFrames[frame].Select(info => info.Touch.PointerInfo.PointerFlags);

    [Theory]
    [InlineData("""{"type":"button","button":0,"down":true}""", 0u, 0x0002u)]
    [InlineData("""{"type":"wheel","dx":0,"dy":120}""", 0u, 0x0800u)]
    [InlineData("""{"type":"key","code":"KeyA","down":true}""", 1u, 0x0008u)]
    public void AppliesInputMessages(string text, uint type, uint flags)
    {
        Receive(text);

        var input = Assert.Single(sent);
        Assert.Equal(type, input.Type);
        Assert.Equal(flags, type == 0 ? input.Data.Mouse.Flags : input.Data.Keyboard.Flags);
        Assert.Empty(logger.Entries);
    }

    [Theory]
    [InlineData("""{"type":"move","x":0.5,"y":0.5}""")]
    [InlineData("""{"type":"moveBy","dx":3,"dy":-2}""")]
    public void MovesTheCursorWithAbsoluteMoves(string text)
    {
        Receive(text);

        // MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE; relative moves would get pointer acceleration.
        Assert.Equal(0x8001u, Assert.Single(sent).Data.Mouse.Flags);
    }

    [Fact]
    public void ReleaseAllReleasesHeldInput()
    {
        Receive("""{"type":"key","code":"ControlLeft","down":true}""");
        Receive("""{"type":"releaseAll"}""");

        Assert.Equal(0x0008u | 0x0002u, sent[^1].Data.Keyboard.Flags);
    }

    [Fact]
    public void AppliesTouchFrames()
    {
        Receive("""{"type":"touch","contacts":[{"id":3,"x":0.5,"y":0.25},{"id":4,"x":1,"y":1}]}""");
        Receive("""{"type":"touch","contacts":[],"canceled":true}""");

        var pointer = touchFrames[0][0].Touch.PointerInfo;
        Assert.Equal((960, 270), (pointer.PixelLocation.X, pointer.PixelLocation.Y));
        Assert.Equal([0x10006u, 0x10006u], TouchFlags(0));
        Assert.Equal([0x48000u, 0x48000u], TouchFlags(1));
        Assert.Empty(sent);
        Assert.Empty(logger.Entries);
    }

    [Fact]
    public void ReleaseAllCancelsTouches()
    {
        Receive("""{"type":"touch","contacts":[{"id":1,"x":0.5,"y":0.5}]}""");
        Receive("""{"type":"releaseAll"}""");

        Assert.Equal([0x48000u], TouchFlags(1));
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void SetsTheStreamCursor(bool visible)
    {
        Receive($$"""{"type":"cursor","visible":{{(visible ? "true" : "false")}}}""");

        Assert.Equal([visible], cursor);
    }

    [Theory]
    [InlineData("""{"type":"scroll"}""")]
    [InlineData("""{"type":null}""")]
    public void WarnsAboutUnknownTypes(string text)
    {
        Receive(text);

        Assert.Empty(sent);
        Assert.Equal(LogLevel.Warning, Assert.Single(logger.Entries).Level);
    }

    [Theory]
    [InlineData("")]
    [InlineData("not json")]
    [InlineData("""{"type":"key","code":"KeyA","down":true""")]
    [InlineData("[]")]
    [InlineData("42")]
    [InlineData("{}")]
    [InlineData("""{"type":5}""")]
    [InlineData("""{"type":"move","x":0.5}""")]
    [InlineData("""{"type":"move","x":"left","y":0.5}""")]
    [InlineData("""{"type":"moveBy","dx":1.5,"dy":0}""")]
    [InlineData("""{"type":"moveBy","dx":1e12,"dy":0}""")]
    [InlineData("""{"type":"button","button":0,"down":"yes"}""")]
    [InlineData("""{"type":"key","code":null,"down":true}""")]
    [InlineData("""{"type":"cursor","visible":1}""")]
    [InlineData("""{"type":"touch"}""")]
    [InlineData("""{"type":"touch","contacts":{}}""")]
    [InlineData("""{"type":"touch","contacts":[{"id":1,"x":"left","y":0.5}]}""")]
    [InlineData("""{"type":"touch","contacts":[{"id":1.5,"x":0.5,"y":0.5}]}""")]
    [InlineData("""{"type":"touch","contacts":[],"canceled":"yes"}""")]
    public void WarnsAboutMalformedMessagesAndKeepsApplyingInput(string text)
    {
        Receive(text);
        Receive("""{"type":"button","button":0,"down":true}""");

        // An error would mean the agent failed, not that the viewer sent a bad message.
        Assert.Equal(LogLevel.Warning, Assert.Single(logger.Entries).Level);
        Assert.Equal(0x0002u, Assert.Single(sent).Data.Mouse.Flags);
    }

    [Fact]
    public void LogsAnErrorWhenApplyingFailsAndKeepsApplyingInput()
    {
        Receive("""{"type":"cursor","visible":false}""", _ => throw new ArgumentException("No capture element"));
        Receive("""{"type":"button","button":0,"down":true}""");

        Assert.Equal(LogLevel.Error, Assert.Single(logger.Entries).Level);
        Assert.Single(sent);
    }

    sealed class ListLogger : ILogger
    {
        public List<(LogLevel Level, string Message)> Entries { get; } = [];

        public void Log<TState>(LogLevel level, EventId id, TState state, Exception? exception, Func<TState, Exception?, string> formatter) =>
            Entries.Add((level, formatter(state, exception)));

        public bool IsEnabled(LogLevel level) => true;

        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;
    }
}
