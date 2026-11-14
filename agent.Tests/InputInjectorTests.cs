using Agent;
using static Agent.InputInjector;

namespace Agent.Tests;

public class InputInjectorTests
{
    // Win32 values, written out so the tests check the injector's constants too.
    const uint Mouse = 0, Keyboard = 1;
    const uint LeftDown = 0x0002, LeftUp = 0x0004, RightDown = 0x0008, RightUp = 0x0010, MiddleUp = 0x0040;
    const uint Wheel = 0x0800, HWheel = 0x1000;
    const uint KeyUp = 0x0002, ScanCode = 0x0008, ExtendedKey = 0x0001;

    readonly List<Input> sent = [];
    readonly InputInjector injector;

    public InputInjectorTests() => injector = new InputInjector(sent.Add);

    IEnumerable<uint> MouseFlags() => sent.Where(input => input.Type == Mouse).Select(input => input.Data.Mouse.Flags);

    [Fact]
    public void SendsAButtonUpOnlyForAHeldButton()
    {
        injector.Button(0, false);
        injector.Button(0, true);
        injector.Button(0, false);
        injector.Button(0, false);

        Assert.Equal([LeftDown, LeftUp], MouseFlags());
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(3)]
    public void IgnoresButtonsOtherThanLeftMiddleAndRight(int button)
    {
        injector.Button(button, true);
        injector.ReleaseAll();

        Assert.Empty(sent);
    }

    [Fact]
    public void SendsRepeatedKeyDownsAndOneUp()
    {
        injector.Key("KeyA", true);
        injector.Key("KeyA", true);
        injector.Key("KeyA", false);
        injector.Key("KeyA", false);

        Assert.All(sent, input => Assert.Equal(Keyboard, input.Type));
        Assert.All(sent, input => Assert.Equal(0x1E, input.Data.Keyboard.ScanCode));
        Assert.Equal([ScanCode, ScanCode, ScanCode | KeyUp], sent.Select(input => input.Data.Keyboard.Flags));
    }

    [Fact]
    public void MarksExtendedKeys()
    {
        injector.Key("ArrowUp", true);

        Assert.Equal(0x48, sent[0].Data.Keyboard.ScanCode);
        Assert.Equal(ScanCode | ExtendedKey, sent[0].Data.Keyboard.Flags);
    }

    [Fact]
    public void SendsPauseAsAVirtualKey()
    {
        injector.Key("Pause", true);
        injector.Key("Pause", false);

        Assert.All(sent, input => Assert.Equal(0x13, input.Data.Keyboard.VirtualKey));
        Assert.Equal([0u, KeyUp], sent.Select(input => input.Data.Keyboard.Flags));
    }

    [Fact]
    public void IgnoresUnknownKeyCodes()
    {
        injector.Key("NotAKey", true);
        injector.ReleaseAll();

        Assert.Empty(sent);
    }

    [Fact]
    public void ReleaseAllReleasesEverythingHeldOnce()
    {
        injector.Key("ShiftLeft", true);
        injector.Button(2, true);
        injector.Button(1, true);
        injector.Button(1, false);
        sent.Clear();

        injector.ReleaseAll();
        injector.ReleaseAll();
        injector.Key("ShiftLeft", false);
        injector.Button(2, false);

        Assert.Equal(2, sent.Count);
        Assert.Equal(ScanCode | KeyUp, sent[0].Data.Keyboard.Flags);
        Assert.Equal([RightUp], MouseFlags());
        Assert.DoesNotContain(MiddleUp, MouseFlags());
    }

    [Fact]
    public void ScrollsWithWindowsVerticalDirectionAndDomHorizontalDirection()
    {
        // DOM dy is positive down; Windows' wheel is positive up.
        injector.Scroll(30, 120);

        Assert.Equal([(Wheel, -120), (HWheel, 30)], sent.Select(input => (input.Data.Mouse.Flags, input.Data.Mouse.MouseData)));
    }

    [Fact]
    public void SkipsScrollAxesWithoutMovement()
    {
        injector.Scroll(0, 0);
        injector.Scroll(0, -120);

        Assert.Equal([(Wheel, 120)], sent.Select(input => (input.Data.Mouse.Flags, input.Data.Mouse.MouseData)));
    }

    [Theory]
    [InlineData(0, 0, 17, 30)]
    [InlineData(1919, 1079, 65518, 65505)]
    [InlineData(960, 540, 32785, 32798)]
    [InlineData(-5, -5, 17, 30)]
    [InlineData(5000, 5000, 65518, 65505)]
    public void AimsAtThePixelsCenterWithinTheDisplay(int x, int y, int expectedX, int expectedY) =>
        Assert.Equal((expectedX, expectedY), AbsoluteCoordinates(x, y, 1920, 1080));

    [Fact]
    public void EveryPixelMapsBackToItself()
    {
        // Windows maps absolute coordinates to pixels as coordinate * size / 65536, truncated.
        foreach (var size in new[] { 1080, 1440, 2160, 2880 })
            for (var pixel = 0; pixel < size; pixel++)
                Assert.Equal(pixel, AbsoluteCoordinates(pixel, 0, size, 1).X * size / 65536);
    }
}
