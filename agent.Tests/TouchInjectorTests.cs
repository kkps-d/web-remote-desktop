using System.Runtime.InteropServices;
using Agent;
using static Agent.TouchInjector;

namespace Agent.Tests;

public class TouchInjectorTests
{
    // Win32 values, written out so the tests check the injector's constants too.
    const uint Down = 0x10000 | 0x2 | 0x4, Update = 0x20000 | 0x2 | 0x4, Up = 0x40000, Canceled = 0x40000 | 0x8000;

    readonly List<PointerTypeInfo[]> frames = [];
    readonly TouchInjector touch;

    public TouchInjectorTests() => touch = new TouchInjector(frames.Add, () => (1920, 1080), Timeout.InfiniteTimeSpan);

    static (uint PointerId, uint Flags, int X, int Y) Describe(PointerTypeInfo info)
    {
        var pointer = info.Touch.PointerInfo;
        return (pointer.PointerId, pointer.PointerFlags, pointer.PixelLocation.X, pointer.PixelLocation.Y);
    }

    IEnumerable<(uint PointerId, uint Flags, int X, int Y)> Frame(int index) => frames[index].Select(Describe);

    [Fact]
    public void MatchesTheWin32StructSizes()
    {
        Assert.Equal(96, Marshal.SizeOf<PointerInfo>());
        Assert.Equal(144, Marshal.SizeOf<PointerTouchInfo>());
        Assert.Equal(152, Marshal.SizeOf<PointerTypeInfo>());
    }

    [Fact]
    public void PressesMovesAndLiftsAContact()
    {
        touch.Update([(7, 0.5, 0.5)]);
        touch.Update([(7, 0.25, 0.75)]);
        touch.Update([]);

        Assert.Equal([(0u, Down, 960, 540)], Frame(0));
        Assert.Equal([(0u, Update, 480, 810)], Frame(1));
        Assert.Equal([(0u, Up, 480, 810)], Frame(2));
        Assert.All(frames.SelectMany(frame => frame), info => Assert.Equal(TouchPointer, info.Type));
        Assert.All(frames.SelectMany(frame => frame), info => Assert.Equal(TouchPointer, info.Touch.PointerInfo.PointerType));
    }

    [Fact]
    public void GivesEveryContactAContactArea()
    {
        touch.Update([(7, 0.5, 0.5)]);

        var info = Assert.Single(frames[0]).Touch;
        Assert.Equal(0x1u, info.TouchMask);
        Assert.Equal((956, 536, 964, 544), (info.Contact.Left, info.Contact.Top, info.Contact.Right, info.Contact.Bottom));
    }

    [Fact]
    public void SendsEveryContactInEachFrame()
    {
        touch.Update([(7, 0.1, 0.1)]);
        touch.Update([(7, 0.1, 0.2), (9, 0.9, 0.9)]);
        touch.Update([(9, 0.8, 0.8)]);

        Assert.Equal([(0u, Update, 192, 216), (1u, Down, 1728, 972)], Frame(1));
        Assert.Equal([(1u, Update, 1536, 864), (0u, Up, 192, 216)], Frame(2));
    }

    [Fact]
    public void ReusesAFreedPointerIdOnlyAfterItsFrame()
    {
        touch.Update([(1, 0, 0)]);
        // Contact 1 lifts while contact 2 lands: 2 can't take pointer id 0 in the same frame.
        touch.Update([(2, 0, 0)]);
        touch.Update([(2, 0, 0), (3, 0, 0)]);

        Assert.Equal([(1u, Down, 0, 0), (0u, Up, 0, 0)], Frame(1));
        Assert.Equal([(1u, Update, 0, 0), (0u, Down, 0, 0)], Frame(2));
    }

    [Fact]
    public void IgnoresContactsPastTheMaximum()
    {
        touch.Update([.. Enumerable.Range(0, MaxContacts + 1).Select(id => (id, 0.5, 0.5))]);

        Assert.Equal(MaxContacts, frames[0].Length);
        Assert.Equal(Enumerable.Range(0, MaxContacts).Select(id => (uint)id), frames[0].Select(info => info.Touch.PointerInfo.PointerId));
    }

    [Fact]
    public void ClampsPositionsToTheDisplay()
    {
        touch.Update([(1, -0.5, 1.5)]);

        Assert.Equal([(0u, Down, 0, 1079)], Frame(0));
    }

    [Fact]
    public void CancelsContactsWhenAsked()
    {
        touch.Update([(1, 0, 0), (2, 1, 1)]);
        touch.Update([], canceled: true);

        Assert.Equal([Canceled, Canceled], frames[1].Select(info => info.Touch.PointerInfo.PointerFlags));
    }

    [Fact]
    public void CancelAllSendsNothingWithoutContacts()
    {
        touch.CancelAll();
        touch.Update([(1, 0, 0)]);
        touch.CancelAll();
        touch.CancelAll();

        Assert.Equal(2, frames.Count);
        Assert.Equal([Canceled], frames[1].Select(info => info.Touch.PointerInfo.PointerFlags));
    }

    [Fact]
    public void KeepAliveResendsHeldContactsOnly()
    {
        touch.KeepAlive();
        touch.Update([(1, 0.5, 0.5)]);
        touch.KeepAlive();
        touch.Update([]);
        touch.KeepAlive();

        Assert.Equal(3, frames.Count);
        Assert.Equal([(0u, Update, 960, 540)], Frame(1));
    }

    [Fact]
    public void KeepAliveTimerResendsHeldContacts()
    {
        var received = new SemaphoreSlim(0);
        using var timed = new TouchInjector(_ => received.Release(), () => (100, 100), TimeSpan.FromMilliseconds(10));

        timed.Update([(1, 0.5, 0.5)]);

        // The press itself, then at least one keep-alive.
        Assert.True(received.Wait(TimeSpan.FromSeconds(5)));
        Assert.True(received.Wait(TimeSpan.FromSeconds(5)));
    }
}
